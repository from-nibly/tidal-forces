use super::*;

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/cast-auth/",
            $name
        ))
        .as_slice()
    };
}

const PEER: &[u8] = fixture!("tls.der");
const REPLY: &[u8] = fixture!("device.reply");
const DEVICE_ROOTS: &[&[u8]] = &[fixture!("root.der")];
const CRL_ROOTS: &[&[u8]] = &[fixture!("crl-root.der")];

fn now() -> u64 {
    std::str::from_utf8(fixture!("clock.txt"))
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

fn trust() -> Trust<'static> {
    Trust {
        device: DEVICE_ROOTS,
        revocation: CRL_ROOTS,
        allow_legacy_device_chain: false,
    }
}

fn context(peer: &[u8]) -> Context {
    let mut context = Context::new(peer).unwrap();
    // Deterministic synthetic signatures only. Production constructor is random.
    context.nonce = std::array::from_fn(|i| i as u8);
    context
}

fn authenticate(bytes: &[u8]) -> Result<Authenticated> {
    context(PEER).authenticate(bytes, &trust(), now())
}

fn change_response(bytes: &[u8], change: impl FnOnce(&mut proto::Response)) -> Vec<u8> {
    let mut envelope = proto::Envelope::decode(bytes).unwrap();
    let mut auth = proto::Auth::decode(envelope.binary.as_deref().unwrap()).unwrap();
    change(auth.response.as_mut().unwrap());
    envelope.binary = Some(auth.encode_to_vec());
    envelope.encode_to_vec()
}

fn change_envelope(change: impl FnOnce(&mut proto::Envelope)) -> Vec<u8> {
    let mut envelope = proto::Envelope::decode(REPLY).unwrap();
    change(&mut envelope);
    envelope.encode_to_vec()
}

fn with_crl(crl: &[u8]) -> Vec<u8> {
    change_response(REPLY, |response| response.crl = Some(crl.to_vec()))
}

#[test]
fn fixed_public_fixture_manifest_is_intact() {
    let manifest: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(fixture!("SHA256.json")).unwrap();
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cast-auth");
    for (name, expected) in &manifest {
        assert_eq!(std::path::Path::new(name).components().count(), 1);
        let bytes = std::fs::read(directory.join(name)).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), *expected, "{name}");
        if name.ends_with(".der") {
            certificate(&bytes).unwrap();
        }
    }
    for entry in std::fs::read_dir(directory).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if name.ends_with(".der")
            || name.ends_with(".reply")
            || name.ends_with(".crl")
            || name == "clock.txt"
        {
            assert!(manifest.contains_key(&name), "unrecorded fixture {name}");
        }
        assert!(!name.ends_with(".key"), "private keys must not be retained");
    }
}

#[test]
fn trusted_signed_nonce_and_tls_binding_produce_bounded_proof() {
    let proof = authenticate(REPLY).unwrap();
    assert_eq!(
        proof.peer_fingerprint,
        <[u8; 32]>::from(Sha256::digest(PEER))
    );
    assert!(!proof.audio_only);
    assert_eq!(proof.valid_until, now() + 3600);
}

#[test]
fn fresh_challenges_request_sha256_and_distinct_random_nonces() {
    let first = Context::new(PEER).unwrap();
    let second = Context::new(PEER).unwrap();
    assert_ne!(first.nonce, second.nonce);
    let envelope = proto::Envelope::decode(first.challenge().as_slice()).unwrap();
    assert_eq!(envelope.source.as_deref(), Some("sender-0"));
    assert_eq!(envelope.destination.as_deref(), Some("receiver-0"));
    let auth = proto::Auth::decode(envelope.binary.as_deref().unwrap()).unwrap();
    let challenge = auth.challenge.unwrap();
    assert_eq!(challenge.hash_algorithm, Some(1));
    assert_eq!(challenge.signature_algorithm, Some(1));
    assert_eq!(challenge.nonce.as_deref(), Some(first.nonce.as_slice()));
}

#[test]
fn recorded_response_cannot_authenticate_a_fresh_challenge() {
    let fresh = Context::new(PEER).unwrap();
    assert_eq!(
        fresh.authenticate(REPLY, &trust(), now()).unwrap_err(),
        Failure::Nonce
    );
}

#[test]
fn nonce_echo_changes_do_not_forge_the_signature() {
    let mut different = context(PEER);
    different.nonce = [99; 16];
    let forged = change_response(REPLY, |r| r.nonce = Some(vec![99; 16]));
    assert_eq!(
        different
            .authenticate(&forged, &trust(), now())
            .unwrap_err(),
        Failure::Signature
    );
}

#[test]
fn another_tls_certificate_cannot_reuse_the_proof() {
    assert_eq!(
        context(fixture!("tls-other.der"))
            .authenticate(REPLY, &trust(), now())
            .unwrap_err(),
        Failure::Signature
    );
}

#[test]
fn corrupted_signature_fails() {
    let forged = change_response(REPLY, |r| r.signature.as_mut().unwrap()[0] ^= 1);
    assert_eq!(authenticate(&forged).unwrap_err(), Failure::Signature);
}

#[test]
fn untrusted_device_chain_is_rejected() {
    let roots = [fixture!("untrusted-root.der")];
    let bad = Trust {
        device: &roots,
        ..trust()
    };
    assert_eq!(
        context(PEER).authenticate(REPLY, &bad, now()).unwrap_err(),
        Failure::Chain
    );
}

#[test]
fn device_and_crl_trust_are_not_interchangeable() {
    let bad = Trust {
        revocation: DEVICE_ROOTS,
        ..trust()
    };
    assert_eq!(
        context(PEER).authenticate(REPLY, &bad, now()).unwrap_err(),
        Failure::Revocation
    );
}

#[test]
fn missing_or_wrong_intermediate_fails() {
    for replacement in [vec![], vec![fixture!("untrusted-root.der").to_vec()]] {
        let forged = change_response(REPLY, |r| r.intermediates = replacement);
        assert_eq!(authenticate(&forged).unwrap_err(), Failure::Chain);
    }
}

#[test]
fn leaf_must_allow_digital_signatures() {
    assert_eq!(
        authenticate(fixture!("wrong-usage-device.reply")).unwrap_err(),
        Failure::Chain
    );
}

#[test]
fn altered_certificate_signatures_and_unknown_critical_extensions_fail() {
    for target in 0..2 {
        let changed = change_response(REPLY, |r| {
            let der = if target == 0 {
                r.certificate.as_mut().unwrap()
            } else {
                &mut r.intermediates[0]
            };
            let last = der.len() - 1;
            der[last] ^= 1;
        });
        assert_eq!(authenticate(&changed).unwrap_err(), Failure::Chain);
    }
    assert_eq!(
        authenticate(fixture!("critical-device.reply")).unwrap_err(),
        Failure::Chain
    );
}

#[test]
fn crl_signer_expiry_limits_the_proof_and_is_enforced() {
    let message = with_crl(fixture!("long-list-short-signer.crl"));
    let proof = authenticate(&message).unwrap();
    let expires = certificate(fixture!("short-crl-signer.der"))
        .unwrap()
        .validity()
        .not_after
        .timestamp() as u64;
    assert_eq!(proof.valid_until, expires);
    assert_eq!(
        context(PEER)
            .authenticate(&message, &trust(), expires + 1)
            .unwrap_err(),
        Failure::Revocation
    );
}

#[test]
fn weak_rsa_leaf_is_not_accepted() {
    assert_eq!(
        authenticate(fixture!("weak-device.reply")).unwrap_err(),
        Failure::Signature
    );
}

#[test]
fn audio_restriction_is_inherited_from_leaf_intermediate_and_root() {
    for reply in [
        fixture!("audio-device.reply"),
        fixture!("audio-chain-device.reply"),
    ] {
        assert!(authenticate(reply).unwrap().audio_only);
    }
    let roots = [fixture!("audio-root.der")];
    let audio = Trust {
        device: &roots,
        ..trust()
    };
    assert!(
        context(PEER)
            .authenticate(fixture!("audio-root-device.reply"), &audio, now())
            .unwrap()
            .audio_only
    );
}

#[test]
fn irrelevant_supplied_certificate_does_not_change_verified_policy() {
    let message = change_response(REPLY, |r| {
        r.intermediates.push(fixture!("audio-root.der").to_vec())
    });
    assert!(!authenticate(&message).unwrap().audio_only);
}

#[test]
fn expired_and_future_tls_certificate_fail_before_acceptance() {
    let start = certificate(PEER).unwrap().validity().not_before.timestamp() as u64;
    for clock in [start - 1, now() + 3 * 86400] {
        assert_eq!(
            context(PEER)
                .authenticate(REPLY, &trust(), clock)
                .unwrap_err(),
            Failure::TlsValidity
        );
    }
}

#[test]
fn excessive_remaining_tls_lifetime_is_rejected_even_with_valid_signature() {
    assert_eq!(
        context(fixture!("tls-long.der"))
            .authenticate(fixture!("long-tls.reply"), &trust(), now())
            .unwrap_err(),
        Failure::TlsValidity
    );
}

#[test]
fn expired_device_certificate_fails() {
    assert_eq!(
        context(PEER)
            .authenticate(fixture!("short-device.reply"), &trust(), now() + 86401)
            .unwrap_err(),
        Failure::Chain
    );
}

#[test]
fn expired_context_fails_without_parsing_reply() {
    let mut old = context(PEER);
    old.started = Instant::now() - Duration::from_secs(11);
    assert_eq!(
        old.authenticate(&[], &trust(), now()).unwrap_err(),
        Failure::Deadline
    );
}

#[test]
fn sha1_unknown_hash_and_pss_have_no_fallback() {
    for hash in [None, Some(0), Some(7)] {
        let message = change_response(REPLY, |r| r.hash_algorithm = hash);
        assert_eq!(authenticate(&message).unwrap_err(), Failure::Algorithm);
    }
    for algorithm in [0, 2, 7] {
        let message = change_response(REPLY, |r| r.signature_algorithm = Some(algorithm));
        assert_eq!(authenticate(&message).unwrap_err(), Failure::Algorithm);
    }
}

#[test]
fn missing_revocation_proof_is_never_optional() {
    let message = change_response(REPLY, |r| r.crl = None);
    assert_eq!(authenticate(&message).unwrap_err(), Failure::Revocation);
}

#[test]
fn signed_revoked_leaf_intermediate_root_and_serial_are_rejected() {
    for crl in [
        fixture!("revoked-device.crl"),
        fixture!("revoked-intermediate.crl"),
        fixture!("revoked-root.crl"),
        fixture!("revoked-serial.crl"),
    ] {
        assert_eq!(
            authenticate(&with_crl(crl)).unwrap_err(),
            Failure::Revocation
        );
    }
}

#[test]
fn serial_outside_revoked_range_is_accepted() {
    assert!(authenticate(&with_crl(fixture!("serial-outside-range.crl"))).is_ok());
}

#[test]
fn signed_but_invalid_crl_semantics_are_rejected() {
    for crl in [
        fixture!("expired.crl"),
        fixture!("future.crl"),
        fixture!("unsupported-version.crl"),
        fixture!("untrusted-signer.crl"),
        fixture!("bad-hash-length.crl"),
        fixture!("too-many-keys.crl"),
        fixture!("invalid-range.crl"),
    ] {
        assert_eq!(
            authenticate(&with_crl(crl)).unwrap_err(),
            Failure::Revocation
        );
    }
}

#[test]
fn tampering_crl_contents_or_signature_fails() {
    for field in 0..2 {
        let mut bundle = proto::CrlBundle::decode(fixture!("clean.crl")).unwrap();
        if field == 0 {
            let list = &mut bundle.lists[0];
            let mut tbs = proto::Crl::decode(list.tbs.as_deref().unwrap()).unwrap();
            tbs.not_after = Some(now() + 100_000);
            list.tbs = Some(tbs.encode_to_vec());
        } else {
            bundle.lists[0].signature.as_mut().unwrap()[0] ^= 1;
        }
        assert_eq!(
            authenticate(&with_crl(&bundle.encode_to_vec())).unwrap_err(),
            Failure::Revocation
        );
    }
}

#[test]
fn empty_or_ambiguous_crl_bundle_is_rejected() {
    let mut bundle = proto::CrlBundle::decode(fixture!("clean.crl")).unwrap();
    bundle.lists.push(bundle.lists[0].clone());
    assert_eq!(
        authenticate(&with_crl(&bundle.encode_to_vec())).unwrap_err(),
        Failure::Revocation
    );
    bundle.lists.clear();
    assert!(authenticate(&with_crl(&bundle.encode_to_vec())).is_err());
}

#[test]
fn wrong_route_payload_and_chunking_are_rejected() {
    for kind in 0..9 {
        let message = change_envelope(|e| match kind {
            0 => e.version = None,
            1 => e.version = Some(1),
            2 => e.source = Some("impostor".into()),
            3 => e.destination = Some("*".into()),
            4 => e.namespace = Some("urn:x-cast:com.google.cast.media".into()),
            5 => e.payload_type = Some(0),
            6 => e.text = Some("not authentication".into()),
            7 => e.continued = Some(true),
            _ => e.remaining = Some(1),
        });
        assert_eq!(authenticate(&message).unwrap_err(), Failure::Envelope);
    }
}

#[test]
fn error_challenge_and_missing_response_fail_closed() {
    for kind in 0..3 {
        let message = change_envelope(|e| {
            let mut auth = proto::Auth::decode(e.binary.as_deref().unwrap()).unwrap();
            match kind {
                0 => auth.error = Some(vec![]),
                1 => auth.challenge = Some(proto::Challenge::default()),
                _ => auth.response = None,
            }
            e.binary = Some(auth.encode_to_vec());
        });
        assert_eq!(authenticate(&message).unwrap_err(), Failure::Malformed);
    }
}

#[test]
fn malformed_and_missing_certificate_nonce_signature_fields_fail() {
    for kind in 0..6 {
        let message = change_response(REPLY, |r| match kind {
            0 => r.certificate = None,
            1 => r.certificate = Some(vec![1]),
            2 => r.signature = None,
            3 => r.signature = Some(vec![]),
            4 => r.nonce = None,
            _ => r.nonce = Some(vec![0; 17]),
        });
        assert!(authenticate(&message).is_err());
    }
}

#[test]
fn input_limits_are_checked_and_der_trailing_bytes_fail() {
    assert_eq!(
        authenticate(&vec![0; MAX_MESSAGE + 1]).unwrap_err(),
        Failure::Bounds
    );
    assert_eq!(authenticate(&[]).unwrap_err(), Failure::Bounds);
    assert!(authenticate(&[0x3a, 0xff, 0xff, 0xff, 0xff, 0x0f]).is_err());
    assert!(Context::new(&vec![0; MAX_CERT + 1]).is_err());
    let mut peer = PEER.to_vec();
    peer.push(0);
    assert!(Context::new(&peer).is_err());
    let too_many = change_response(REPLY, |r| {
        r.intermediates = vec![fixture!("intermediate.der").to_vec(); 5]
    });
    assert_eq!(authenticate(&too_many).unwrap_err(), Failure::Bounds);
    let huge_crl = change_response(REPLY, |r| r.crl = Some(vec![0; MAX_CRL + 1]));
    assert_eq!(authenticate(&huge_crl).unwrap_err(), Failure::Bounds);
    let empty = Trust {
        device: &[],
        ..trust()
    };
    assert_eq!(
        context(PEER)
            .authenticate(REPLY, &empty, now())
            .unwrap_err(),
        Failure::Bounds
    );
}

#[test]
fn every_truncated_encoded_response_fails_closed() {
    for end in 0..REPLY.len() {
        assert!(
            authenticate(&REPLY[..end]).is_err(),
            "accepted prefix {end}"
        );
    }
}
