use std::convert::TryInto;

use anyhow::{Context, Result};
use futures::{Sink, Stream, StreamExt, TryStreamExt};
use log::{debug, error, info};
use round_based::{Incoming, Outgoing};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct IssuedUniqueIdx {
    unique_idx: u16,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Msg<M> {
    sender: u16,
    receiver: Option<u16>,
    body: M,
}

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("Failed to serialize/deserialize message: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Network communication error: {0}")]
    Network(#[from] anyhow::Error),

    #[error("HTTP client error: {0}")]
    Http(String),

    #[error("Server-Sent Events stream error: {0}")]
    Sse(String),

    #[error("Failed to issue unique party index")]
    IndexIssuance,

    #[error("Failed to broadcast message to room")]
    Broadcast,

    #[error("Failed to subscribe to message stream")]
    Subscription,

    #[error("Invalid message format received")]
    InvalidMessage,

    #[error("Connection to room '{room_id}' failed")]
    ConnectionFailed { room_id: String },
}

#[derive(Clone, Debug)]
pub struct Client {
    client: surf::Client,
}

impl Client {
    pub fn new(address: surf::Url) -> Result<Self> {
        info!("Creating new client for address: {}", address);
        Ok(Self {
            client: surf::Config::new()
                .set_base_url(address)
                .set_timeout(None)
                .try_into()?,
        })
    }

    pub fn room(&self, room: &str) -> Room {
        Room::new(self.client.clone(), room.to_string())
    }
}

#[derive(Clone)]
pub struct Room {
    client: surf::Client,
    room: String,
}

impl Room {
    pub fn new(client: surf::Client, room: String) -> Self {
        Room {
            client,
            room: format!("rooms/{}", room),
        }
    }

    fn endpoint(&self, endpoint: &str) -> String {
        format!("{}/{}", self.room, endpoint)
    }

    #[allow(dead_code)]
    async fn issue_index(&self) -> Result<u16, TransportError> {
        let endpoint = self.endpoint("issue_unique_idx");
        debug!("Requesting unique index from endpoint: {}", endpoint);
        let response = self
            .client
            .post(endpoint)
            .recv_json::<IssuedUniqueIdx>()
            .await
            .map_err(|e| {
                let err =
                    TransportError::Http(format!("Failed to issue index: {}", e.into_inner()));
                error!("Failed to issue index: {}", err);
                err
            })?;
        info!("Issued unique index: {}", response.unique_idx);
        Ok(response.unique_idx)
    }

    async fn broadcast(&self, message: &str) -> Result<(), TransportError> {
        let endpoint = self.endpoint("broadcast");
        debug!("Broadcasting message to endpoint: {}", endpoint);
        self.client
            .post(endpoint)
            .body(message)
            .await
            .map_err(|e| {
                let err = TransportError::Http(format!(
                    "Failed to broadcast message: {}",
                    e.into_inner()
                ));
                error!("Failed to broadcast message: {}", err);
                err
            })?;
        debug!("Message broadcast successful");
        Ok(())
    }

    async fn subscribe(
        &self,
    ) -> Result<
        std::pin::Pin<Box<dyn Stream<Item = Result<String, anyhow::Error>> + Send>>,
        TransportError,
    > {
        let endpoint = self.endpoint("subscribe");
        debug!("Subscribing to SSE stream at endpoint: {}", endpoint);
        let response = self.client.get(endpoint).await.map_err(|e| {
            let err =
                TransportError::Http(format!("Failed to subscribe to stream: {}", e.into_inner()));
            error!("Failed to subscribe to stream: {}", err);
            err
        })?;
        let events = async_sse::decode(response);
        let stream = events.filter_map(|msg| {
            Box::pin(async {
                match msg {
                    Ok(async_sse::Event::Message(msg)) => Some(
                        String::from_utf8(msg.into_bytes())
                            .context("Received invalid UTF-8 in SSE message"),
                    ),
                    Ok(_) => {
                        // ignore other types of SSE events (like comments, etc.)
                        None
                    }
                    Err(e) => {
                        let err = anyhow::Error::new(TransportError::Sse(format!(
                            "SSE stream error: {}",
                            e.into_inner()
                        )));
                        error!("SSE stream error: {}", err);
                        Some(Err(err))
                    }
                }
            })
        });
        Ok(Box::pin(stream))
    }

    pub async fn join_room<M>(
        self,
        index: u16,
    ) -> Result<(
        u16,
        std::pin::Pin<Box<dyn Stream<Item = Result<Incoming<M>, TransportError>> + Send>>,
        std::pin::Pin<Box<dyn Sink<Outgoing<M>, Error = TransportError> + Send>>,
    )>
    where
        M: Serialize + DeserializeOwned + Send + 'static,
    {
        let room = self.room.clone();

        // Clone what we need for the closures
        let outgoing_client = self.client.clone();

        // Construct channel of incoming messages
        let incoming = self
            .subscribe()
            .await?
            .map_err(TransportError::Network)
            .and_then(|msg| {
                Box::pin(async move {
                    serde_json::from_str::<Msg<M>>(&msg).map_err(TransportError::from)
                })
            });

        // Ignore incoming messages addressed to someone else
        let incoming = incoming.try_filter(move |msg| {
            let should_receive =
                msg.sender != index && (msg.receiver.is_none() || msg.receiver == Some(index));
            if !should_receive {
                debug!(
                    "Ignoring message from sender {} to receiver {:?} (our index: {})",
                    msg.sender, msg.receiver, index
                );
            }
            futures::future::ready(should_receive)
        });

        // Convert Msg<M> to Incoming<M>
        let incoming = incoming.map_ok(|msg| Incoming {
            id: 0,
            sender: msg.sender,
            msg_type: if msg.receiver.is_none() {
                round_based::MessageType::Broadcast
            } else {
                round_based::MessageType::P2P
            },
            msg: msg.body,
        });

        // Pin the incoming stream
        let incoming = Box::pin(incoming);

        // Construct channel of outgoing messages
        let outgoing =
            futures::sink::unfold(outgoing_client, move |client, message: Outgoing<M>| {
                let room = self.clone();
                Box::pin(async move {
                    let msg = Msg {
                        sender: index,
                        receiver: match message.recipient {
                            round_based::MessageDestination::AllParties => None,
                            round_based::MessageDestination::OneParty(party_id) => Some(party_id),
                        },
                        body: message.msg,
                    };
                    let serialized = serde_json::to_string(&msg).map_err(TransportError::from)?;
                    room.broadcast(&serialized).await.map_err(|e| {
                        error!("Failed to broadcast outgoing message: {}", e);
                        e
                    })?;
                    Ok::<_, TransportError>(client)
                })
            });

        // Pin the outgoing sink
        let outgoing = Box::pin(outgoing);

        info!("Successfully joined room '{room}' with index {index}");

        Ok((index, incoming, outgoing))
    }
}
