use std::iter;

use cggmp24_tests::external_verifier::ExternalVerifier;
use generic_ec::{coords::HasAffineX, Curve, Point};
use rand::seq::SliceRandom;
use rand::{Rng, RngCore};
use rand_dev::DevRng;
use sha2::Sha256;

use cggmp24::key_share::AnyKeyShare;
use cggmp24::signing::DataToSign;
use cggmp24::ExecutionId;

cggmp24_tests::test_suite! {
    test: signing_works,
    generics: all_curves,
    suites: {
        n2: (None, 2, false, false),
        n2_reliable: (None, 2, true, false),
        t2n2: (Some(2), 2, false, false),
        n3: (None, 3, false, false),
        t2n3: (Some(2), 3, false, false),
        t3n3: (Some(3), 3, false, false),

        #[cfg(feature = "hd-wallet")]
        n3_hd: (None, 3, false, true),
        #[cfg(feature = "hd-wallet")]
        t2n3_hd: (Some(2), 3, false, true),
        #[cfg(feature = "hd-wallet")]
        t3n3_hd: (Some(3), 3, false, true),
    }
}

fn signing_works<E>(t: Option<u16>, n: u16, reliable_broadcast: bool, hd_wallet: bool)
where
    E: Curve + cggmp24_tests::CurveParams,
    Point<E>: HasAffineX<E>,
{
    #[cfg(not(feature = "hd-wallet"))]
    assert!(!hd_wallet);

    let mut rng = DevRng::new();

    let shares = cggmp24_tests::CACHED_SHARES
        .get_shares::<E>(t, n, hd_wallet)
        .expect("retrieve cached shares");

    let eid: [u8; 32] = rng.gen();
    let eid = ExecutionId::new(&eid);

    let mut original_message_to_sign = [0u8; 100];
    rng.fill_bytes(&mut original_message_to_sign);
    let message_to_sign = DataToSign::digest::<Sha256>(&original_message_to_sign);

    #[cfg(feature = "hd-wallet")]
    let derivation_path = if hd_wallet {
        Some(cggmp24_tests::random_derivation_path(&mut rng))
    } else {
        None
    };

    // Choose `t` signers to perform signing
    let t = shares[0].min_signers();
    let mut participants = (0..n).collect::<Vec<_>>();
    participants.shuffle(&mut rng);
    let participants = &participants[..usize::from(t)];
    println!("Signers: {participants:?}");
    let participants_shares = participants.iter().map(|i| &shares[usize::from(*i)]);

    let sig = round_based::sim::run_with_setup(participants_shares, |i, party, share| {
        let party = cggmp24_tests::buffer_outgoing(party);
        let mut party_rng = rng.fork();

        let signing = cggmp24::signing(eid, i, participants, share)
            .enforce_reliable_broadcast(reliable_broadcast);

        #[cfg(feature = "hd-wallet")]
        let signing = if let Some(derivation_path) = derivation_path.clone() {
            signing
                .set_derivation_path_with_algo::<E::HdAlgo, _>(derivation_path)
                .unwrap()
        } else {
            signing
        };

        async move { signing.sign(&mut party_rng, party, message_to_sign).await }
    })
    .unwrap()
    .expect_ok()
    .expect_eq();

    #[cfg(feature = "hd-wallet")]
    let public_key = if let Some(path) = &derivation_path {
        generic_ec::NonZero::from_point(
            shares[0]
                .derive_child_public_key::<E::HdAlgo, _>(path.iter().cloned())
                .unwrap()
                .public_key,
        )
        .unwrap()
    } else {
        shares[0].shared_public_key
    };
    #[cfg(not(feature = "hd-wallet"))]
    let public_key = shares[0].shared_public_key;

    sig.verify(&public_key, &message_to_sign)
        .expect("signature is not valid");

    E::ExVerifier::verify(&public_key, &sig, &original_message_to_sign)
        .expect("external verification failed")
}

cggmp24_tests::test_suite! {
    test: signing_with_presigs,
    generics: all_curves,
    suites: {
        t3n5: (Some(3), 5, false),
        #[cfg(feature = "hd-wallet")]
        t3n5_hd: (Some(3), 5, false),
    }
}

fn signing_with_presigs<E>(t: Option<u16>, n: u16, hd_wallet: bool)
where
    E: Curve + cggmp24_tests::CurveParams,
    Point<E>: HasAffineX<E>,
{
    #[cfg(not(feature = "hd-wallet"))]
    assert!(!hd_wallet);

    let mut rng = DevRng::new();

    let shares = cggmp24_tests::CACHED_SHARES
        .get_shares::<E>(t, n, hd_wallet)
        .expect("retrieve cached shares");

    let eid: [u8; 32] = rng.gen();
    let eid = ExecutionId::new(&eid);

    // Choose `t` signers to generate presignature
    let t = shares[0].min_signers();
    let mut participants = (0..n).collect::<Vec<_>>();
    participants.shuffle(&mut rng);
    let participants = &participants[..usize::from(t)];
    println!("Signers: {participants:?}");

    let participants_shares = participants.iter().map(|i| &shares[usize::from(*i)]);

    let presigs = round_based::sim::run_with_setup(participants_shares, |i, party, share| {
        let party = cggmp24_tests::buffer_outgoing(party);
        let mut party_rng = rng.fork();

        async move {
            cggmp24::signing(eid, i, participants, share)
                .generate_presignature(&mut party_rng, party)
                .await
        }
    })
    .unwrap()
    .expect_ok()
    .into_vec();

    // Now, that we have presignatures generated, we learn (generate) a messages to sign
    // and the derivation path (if hd is enabled)
    let mut original_message_to_sign = [0u8; 100];
    rng.fill_bytes(&mut original_message_to_sign);
    let message_to_sign = DataToSign::digest::<Sha256>(&original_message_to_sign);

    #[cfg(feature = "hd-wallet")]
    let derivation_path = if hd_wallet {
        Some(cggmp24_tests::random_derivation_path(&mut rng))
    } else {
        None
    };

    // all presig commitments must be same
    for (i, (_, commitment)) in presigs.iter().enumerate() {
        assert_eq!(presigs[0].1, *commitment, "cmp(0, {i})")
    }
    let (_, commitments) = presigs[0].clone();

    let partial_signatures = presigs
        .into_iter()
        .map(|(presig, _commitments)| {
            #[cfg(feature = "hd-wallet")]
            let presig = if let Some(derivation_path) = &derivation_path {
                let epub = shares[0].extended_public_key().expect("not hd wallet");
                presig
                    .set_derivation_path_with_algo::<E::HdAlgo, _>(
                        epub,
                        derivation_path.iter().copied(),
                    )
                    .unwrap()
            } else {
                presig
            };
            presig.issue_partial_signature(message_to_sign)
        })
        .collect::<Vec<_>>();

    let signature =
        cggmp24::PartialSignature::combine(&partial_signatures, &commitments, message_to_sign)
            .expect("invalid partial sigantures");

    #[cfg(feature = "hd-wallet")]
    let public_key = if let Some(path) = &derivation_path {
        generic_ec::NonZero::from_point(
            shares[0]
                .derive_child_public_key::<E::HdAlgo, _>(path.iter().cloned())
                .unwrap()
                .public_key,
        )
        .unwrap()
    } else {
        shares[0].shared_public_key
    };
    #[cfg(not(feature = "hd-wallet"))]
    let public_key = shares[0].shared_public_key;

    signature
        .verify(&public_key, &message_to_sign)
        .expect("signature is not valid");

    E::ExVerifier::verify(&public_key, &signature, &original_message_to_sign)
        .expect("external verification failed")
}

cggmp24_tests::test_suite! {
    test: signing_sync,
    generics: all_curves,
    suites: {
        n3: (None, 3, false),
        t3n5: (Some(3), 5, false),
        #[cfg(feature = "hd-wallet")]
        n3_hd: (None, 3, true),
        #[cfg(feature = "hd-wallet")]
        t3n5_hd: (Some(3), 5, true),
    }
}

fn signing_sync<E>(t: Option<u16>, n: u16, hd_wallet: bool)
where
    E: Curve + cggmp24_tests::CurveParams,
    Point<E>: HasAffineX<E>,
{
    #[cfg(not(feature = "hd-wallet"))]
    assert!(!hd_wallet);

    let mut rng = DevRng::new();

    let shares = cggmp24_tests::CACHED_SHARES
        .get_shares::<E>(t, n, hd_wallet)
        .expect("retrieve cached shares");

    let eid: [u8; 32] = rng.gen();
    let eid = ExecutionId::new(&eid);

    let mut original_message_to_sign = [0u8; 100];
    rng.fill_bytes(&mut original_message_to_sign);
    let message_to_sign = DataToSign::digest::<Sha256>(&original_message_to_sign);

    #[cfg(feature = "hd-wallet")]
    let derivation_path = if hd_wallet {
        Some(cggmp24_tests::random_derivation_path(&mut rng))
    } else {
        None
    };

    // Choose `t` signers to perform signing
    let t = shares[0].min_signers();
    let mut participants = (0..n).collect::<Vec<_>>();
    participants.shuffle(&mut rng);
    let participants = &participants[..usize::from(t)];
    println!("Signers: {participants:?}");
    let participants_shares = participants.iter().map(|i| &shares[usize::from(*i)]);

    let mut signer_rng = iter::repeat_with(|| rng.fork())
        .take(n.into())
        .collect::<Vec<_>>();

    let mut simulation = round_based::sim::Simulation::with_capacity(n);

    for ((i, share), signer_rng) in (0..).zip(participants_shares).zip(&mut signer_rng) {
        simulation.add_party({
            let signing = cggmp24::signing(eid, i, participants, share);

            #[cfg(feature = "hd-wallet")]
            let signing = if let Some(derivation_path) = derivation_path.clone() {
                signing
                    .set_derivation_path_with_algo::<E::HdAlgo, _>(derivation_path)
                    .unwrap()
            } else {
                signing
            };

            signing.sign_sync(signer_rng, message_to_sign)
        })
    }

    let sig = simulation.run().unwrap().expect_ok().expect_eq();

    #[cfg(feature = "hd-wallet")]
    let public_key = if let Some(path) = &derivation_path {
        generic_ec::NonZero::from_point(
            shares[0]
                .derive_child_public_key::<E::HdAlgo, _>(path.iter().cloned())
                .unwrap()
                .public_key,
        )
        .unwrap()
    } else {
        shares[0].shared_public_key
    };
    #[cfg(not(feature = "hd-wallet"))]
    let public_key = shares[0].shared_public_key;

    sig.verify(&public_key, &message_to_sign)
        .expect("signature is not valid");

    E::ExVerifier::verify(&public_key, &sig, &original_message_to_sign)
        .expect("external verification failed")
}
