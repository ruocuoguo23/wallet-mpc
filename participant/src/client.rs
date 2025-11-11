use tokio_util::compat::TokioAsyncReadCompatExt;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::{Sink, Stream, StreamExt, TryStreamExt};
use log::{debug, error, info};
use round_based::{Incoming, Outgoing};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;
use tokio_util::io::StreamReader;

#[allow(unused_imports)]
use tokio_util::compat::FuturesAsyncReadCompatExt;

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
    client: reqwest::Client,
    base_url: String,
}

impl Client {
    pub fn new(address: reqwest::Url) -> Result<Self> {
        info!("Creating new client for address: {}", address);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))  // 完全禁用超时，SSE 需要长连接
            .tcp_keepalive(Duration::from_secs(60))  // tcp_keepalive 接受 Duration，不是 Option
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self {
            client,
            base_url: address.to_string(),
        })
    }

    pub fn room(&self, room: &str) -> Room {
        Room::new(self.client.clone(), self.base_url.clone(), room.to_string())
    }
}

#[derive(Clone)]
pub struct Room {
    client: reqwest::Client,
    base_url: String,
    room: String,
}

impl Room {
    pub fn new(client: reqwest::Client, base_url: String, room: String) -> Self {
        Room {
            client,
            base_url,
            room: format!("rooms/{}", room),
        }
    }

    fn endpoint(&self, endpoint: &str) -> String {
        format!("{}/{}/{}", self.base_url.trim_end_matches('/'), self.room, endpoint)
    }

    #[allow(dead_code)]
    async fn issue_index(&self) -> Result<u16, TransportError> {
        let endpoint = self.endpoint("issue_unique_idx");
        debug!("Requesting unique index from endpoint: {}", endpoint);

        let response = self
            .client
            .post(&endpoint)
            .send()
            .await
            .map_err(|e| {
                let err = TransportError::Http(format!("Failed to issue index: {}", e));
                error!("{}", err);
                err
            })?;

        let issued_idx = response
            .json::<IssuedUniqueIdx>()
            .await
            .map_err(|e| {
                let err = TransportError::Http(format!("Failed to parse index response: {}", e));
                error!("{}", err);
                err
            })?;

        info!("Issued unique index: {}", issued_idx.unique_idx);
        Ok(issued_idx.unique_idx)
    }

    async fn broadcast(&self, message: &str) -> Result<(), TransportError> {
        let endpoint = self.endpoint("broadcast");
        debug!("Broadcasting message to endpoint: {}", endpoint);

        self.client
            .post(&endpoint)
            .body(message.to_string())
            .send()
            .await
            .map_err(|e| {
                let err = TransportError::Http(format!("Failed to broadcast message: {}", e));
                error!("{}", err);
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

        let response = self.client
            .get(&endpoint)
            .header("Accept", "text/event-stream")  // 明确接受 SSE
            .header("Cache-Control", "no-cache")     // 禁用缓存
            .header("Connection", "keep-alive")      // 保持连接
            .send()
            .await
            .map_err(|e| {
                let err = TransportError::Http(format!("Failed to subscribe to stream: {}", e));
                error!("{}", err);
                err
            })?;

        // Convert the response body into a byte stream
        let byte_stream = response.bytes_stream();

        // Convert Stream<Item = Result<Bytes, Error>> to AsyncRead
        let byte_stream_mapped = byte_stream.map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e)
        });

        let stream_reader = StreamReader::new(byte_stream_mapped);
        let async_read = stream_reader.compat();

        // Use async-sse to decode SSE events
        let events = async_sse::decode(async_read);

        let stream = events.filter_map(|msg| {
            Box::pin(async {
                match msg {
                    Ok(async_sse::Event::Message(msg)) => {
                        Some(
                            String::from_utf8(msg.into_bytes())
                                .context("Received invalid UTF-8 in SSE message")
                        )
                    }
                    Ok(_) => {
                        // ignore other types of SSE events (like comments, etc.)
                        None
                    }
                    Err(e) => {
                        let err = anyhow::Error::new(TransportError::Sse(format!(
                            "SSE stream error: {}",
                            e
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
        let outgoing_room = self.clone();

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
                let room = outgoing_room.clone();
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
