use super::*;
use rustls::{ServerConfig, ServerConnection};
use rustls_pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use std::net::TcpListener;

#[test]
fn embedded_roots_match_reviewed_provenance_and_are_distinct() {
    for (der, digest) in [
        (
            CAST,
            "809af14700b3fe2611ad597eb1584d6354313b64ccdb3390f097fa3e5826d6ca",
        ),
        (
            EUREKA,
            "caf6d1e37b532203e01a76bf07187bb731ccd38801565ab2211a2c0ab7f3bc46",
        ),
        (
            CRL,
            "8b77c60291f9d62764613083f49750b1c5caac2d9efbe5a28edf301f9d84831d",
        ),
    ] {
        assert_eq!(format!("{:x}", Sha256::digest(der)), digest);
        expiry(&certificate(der).unwrap(), clock().unwrap()).unwrap();
        anchor_from_trusted_cert(&CertificateDer::from(der)).unwrap();
    }
    assert_ne!(CAST, CRL);
    assert_ne!(CAST, EUREKA);
}

#[test]
fn private_fixture_does_not_authenticate_against_official_roots() {
    let leaf = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/cast-auth/device.der"
    ));
    let intermediate = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/cast-auth/intermediate.der"
    ));
    let clock: u64 = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/cast-auth/clock.txt"
    ))
    .trim()
    .parse()
    .unwrap();
    assert_eq!(
        verify_chain(
            leaf,
            &[intermediate.to_vec()],
            official_trust().device,
            clock
        )
        .unwrap_err(),
        Failure::Chain
    );
}

#[test]
fn legacy_policy_requires_an_exact_explicit_flag() {
    let args = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(parse_mode(&args(&["--authenticate-ip-stdin"])), Ok(false));
    assert_eq!(
        parse_mode(&args(&[
            "--authenticate-ip-stdin",
            "--allow-legacy-device-certificates"
        ])),
        Ok(true)
    );
    for values in [
        vec![],
        vec!["--allow-legacy-device-certificates"],
        vec!["--authenticate-ip-stdin", "--skip-verification"],
        vec![
            "--authenticate-ip-stdin",
            "--allow-legacy-device-certificates",
            "extra",
        ],
    ] {
        assert!(parse_mode(&args(&values)).is_err());
    }
}

#[test]
fn audible_test_requires_exact_volume_opt_in() {
    let args = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(tone_mode(&args(&[
        "--tone-test-ip-stdin",
        "--allow-legacy-device-certificates",
        "--volume-percent=100"
    ])));
    for values in [
        vec![],
        vec!["--tone-test-ip-stdin"],
        vec!["--tone-test-ip-stdin", "--allow-legacy-device-certificates"],
        vec![
            "--tone-test-ip-stdin",
            "--allow-legacy-device-certificates",
            "--volume-percent=50",
        ],
    ] {
        assert!(!tone_mode(&args(&values)));
        assert!(parse_mode(&args(&values)).is_err());
    }
}

#[test]
fn network_probe_rejects_unapproved_address_classes_without_connecting() {
    for value in [
        "127.0.0.1",
        "169.254.1.2",
        "8.8.8.8",
        "224.0.0.1",
        "::1",
        "255.255.255.255",
    ] {
        assert!(inspect_ip(value.parse().unwrap(), false).is_err());
    }
}

#[test]
fn inline_dash_cli_requires_exact_volume_and_explicit_legacy_opt_in() {
    let args = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(dash_mode(&args(&[
        "--dash-data-url-test-ip-stdin",
        "--volume-percent=50"
    ])));
    assert!(!dash_legacy_mode(&args(&[
        "--dash-data-url-test-ip-stdin",
        "--volume-percent=50"
    ])));
    let legacy = args(&[
        "--dash-data-url-test-ip-stdin",
        "--allow-legacy-device-certificates",
        "--volume-percent=50",
    ]);
    assert!(dash_mode(&legacy) && dash_legacy_mode(&legacy));
    assert!(!dash_full_mode(&legacy));
    let full = args(&["--dash-data-url-test-ip-stdin", "--volume-percent=100"]);
    assert!(dash_mode(&full) && dash_full_mode(&full) && !dash_legacy_mode(&full));
    let full_legacy = args(&[
        "--dash-data-url-test-ip-stdin",
        "--allow-legacy-device-certificates",
        "--volume-percent=100",
    ]);
    assert!(
        dash_mode(&full_legacy) && dash_full_mode(&full_legacy) && dash_legacy_mode(&full_legacy)
    );
    for values in [
        vec!["--dash-data-url-test-ip-stdin"],
        vec!["--dash-data-url-test-ip-stdin", "--volume-percent=101"],
        vec![
            "--dash-data-url-test-ip-stdin",
            "--volume-percent=50",
            "--allow-legacy-device-certificates",
        ],
    ] {
        assert!(!dash_mode(&args(&values)));
        assert!(!tone_mode(&args(&values)));
        assert!(parse_mode(&args(&values)).is_err());
    }
}

#[test]
#[ignore = "requires tests/cast-auth.py private ephemeral key runner; never a live device"]
fn private_tls_contract() {
    let root = std::path::PathBuf::from(
        std::env::var_os("TIDAL_CAST_PRIVATE_FIXTURES").expect("private runner required"),
    );
    let fixture = |name: &str| std::fs::read(root.join("fixtures").join(name)).unwrap();
    let device_root = fixture("root.der");
    let crl_root = fixture("crl-root.der");
    let device_roots = [device_root.as_slice()];
    let crl_roots = [crl_root.as_slice()];
    let trust = Trust {
        device: &device_roots,
        revocation: &crl_roots,
        allow_legacy_device_chain: false,
    };
    for mode in 0..29 {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let tls_cert = fixture("tls.der");
        let tls_key = std::fs::read(root.join("keys/tls.pk8")).unwrap();
        let device_key = std::fs::read(root.join("keys/device.pk8")).unwrap();
        let template = fixture(if (6..=12).contains(&mode) || mode >= 14 {
            "sha1-device.reply"
        } else {
            "device.reply"
        });
        let revoked = fixture("revoked-device.crl");
        let other_tls = fixture("tls-other.der");
        let server = std::thread::spawn(move || {
            let versions = [if mode == 1 {
                &rustls::version::TLS12
            } else {
                &rustls::version::TLS13
            }];
            let config = ServerConfig::builder_with_provider(Arc::new(default_provider()))
                .with_protocol_versions(&versions)
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    vec![CertificateDer::from(tls_cert.clone())],
                    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(tls_key)),
                )
                .unwrap();
            let (socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut stream =
                StreamOwned::new(ServerConnection::new(Arc::new(config)).unwrap(), socket);
            let mut header = [0; 4];
            stream.read_exact(&mut header).unwrap();
            let count = u32::from_be_bytes(header) as usize;
            assert!(count < 1024);
            let mut bytes = vec![0; count];
            stream.read_exact(&mut bytes).unwrap();
            let env = proto::Envelope::decode(bytes.as_slice()).unwrap();
            assert_eq!(env.namespace.as_deref(), Some(NAMESPACE));
            assert_eq!(env.source.as_deref(), Some("sender-0"));
            assert_eq!(env.destination.as_deref(), Some("receiver-0"));
            let auth = proto::Auth::decode(env.binary.as_deref().unwrap()).unwrap();
            assert!(auth.response.is_none() && auth.error.is_none());
            let challenge = auth.challenge.unwrap();
            assert_eq!(challenge.hash_algorithm, Some(1));
            assert_eq!(challenge.signature_algorithm, Some(1));
            let nonce = challenge.nonce.unwrap();
            assert_eq!(nonce.len(), 16);
            let mut env = proto::Envelope::decode(template.as_slice()).unwrap();
            let mut auth = proto::Auth::decode(env.binary.as_deref().unwrap()).unwrap();
            let response = auth.response.as_mut().unwrap();
            response.nonce = Some(nonce.clone());
            let mut message = nonce;
            message.extend_from_slice(if mode == 4 { &other_tls } else { &tls_cert });
            let pair = ring::signature::RsaKeyPair::from_pkcs8(&device_key).unwrap();
            let mut signature = vec![0; pair.public().modulus_len()];
            pair.sign(
                &ring::signature::RSA_PKCS1_SHA256,
                &ring::rand::SystemRandom::new(),
                &message,
                &mut signature,
            )
            .unwrap();
            if mode == 2 || mode == 9 || mode == 11 {
                signature[0] ^= 1;
            }
            if mode == 3 || mode == 8 {
                response.crl = Some(revoked);
            }
            response.signature = Some(signature);
            if mode == 10 {
                response.hash_algorithm = Some(0);
            }
            env.binary = Some(auth.encode_to_vec());
            let bytes = env.encode_to_vec();
            stream
                .write_all(
                    &(if mode == 5 {
                        u32::MAX
                    } else {
                        bytes.len() as u32
                    })
                    .to_be_bytes(),
                )
                .unwrap();
            if mode != 5 {
                stream.write_all(&bytes).unwrap();
            }
            stream.flush().unwrap();
            if mode == 12 || mode == 13 || mode >= 15 {
                tone::tests::serve(
                    &mut stream,
                    if mode >= 22 {
                        tone::Fixture::WholeTrack
                    } else if mode >= 16 {
                        tone::Fixture::InlineDashFullVolume
                    } else if mode >= 13 {
                        tone::Fixture::InlineDash
                    } else {
                        tone::Fixture::Tone
                    },
                    match mode {
                        17 => Some("LAUNCH"),
                        18 => Some("LOAD"),
                        19 => Some("STATUS_HEADER"),
                        20 => Some("STATUS_PAYLOAD"),
                        21 => Some("STATUS_MISSING"),
                        23 => Some("FINISHED_EVENT"),
                        24 => Some("EMPTY_ONLY"),
                        25 => Some("ERROR_EVENT"),
                        26 => Some("CANCELLED_EVENT"),
                        27 => Some("FOREIGN_EVENT"),
                        28 => Some("STALE_EVENT"),
                        _ => None,
                    },
                );
            }
            let mut extra = [0; 1];
            match stream.read(&mut extra) {
                Ok(0) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::UnexpectedEof
                            | io::ErrorKind::ConnectionReset
                            | io::ErrorKind::BrokenPipe
                    ) => {}
                other => panic!("probe did not close without further application bytes: {other:?}"),
            }
        });
        let cancel = AtomicBool::new(false);
        let scoped_trust = Trust {
            allow_legacy_device_chain: (7..=12).contains(&mode) || mode >= 15,
            ..trust
        };
        let result = run_target(
            address,
            &scoped_trust,
            &cancel,
            Duration::from_secs(2),
            (mode >= 11).then_some(if mode >= 22 {
                tone::Fixture::WholeTrack
            } else if mode >= 16 {
                tone::Fixture::InlineDashFullVolume
            } else if mode >= 13 {
                tone::Fixture::InlineDash
            } else {
                tone::Fixture::Tone
            }),
        );
        if mode < 2
            || mode == 7
            || mode == 12
            || mode == 13
            || (15..=18).contains(&mode)
            || mode == 22
            || mode == 23
        {
            assert!(result.is_ok(), "{result:?}");
        } else {
            let expected = match mode {
                19..=21 => Failure::Media("Receiver reply timed out"),
                24 | 27 | 28 => Failure::Media("Natural media completion was not confirmed"),
                25 => Failure::Media("Receiver reported a playback error"),
                26 => Failure::Media("Receiver reported cancelled playback"),
                2 | 4 | 9 | 11 => Failure::Signature,
                3 | 8 => Failure::Revocation,
                10 => Failure::Algorithm,
                6 | 14 => {
                    Failure::CertificateAlgorithm("Rejected certificate algorithm: RSA/SHA-1")
                }
                _ => Failure::Bounds,
            };
            assert_eq!(result.unwrap_err(), expected);
        }
        server.join().unwrap();
    }
    for cancel_during in [false, true] {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let server_cancel = Arc::clone(&cancel);
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut buffer = [0; 1024];
            stream.read_exact(&mut buffer[..1]).unwrap();
            if cancel_during {
                server_cancel.store(true, Ordering::Relaxed);
            }
            while matches!(stream.read(&mut buffer), Ok(n) if n > 0) {}
        });
        let began = Instant::now();
        let result = authenticate_target(address, &trust, &cancel, Duration::from_millis(250));
        assert_eq!(
            result.unwrap_err(),
            if cancel_during {
                Failure::Cancelled
            } else {
                Failure::Deadline
            }
        );
        assert!(began.elapsed() < Duration::from_secs(1));
        server.join().unwrap();
    }
    let cancel = AtomicBool::new(true);
    assert_eq!(
        authenticate_target(
            "127.0.0.1:9".parse().unwrap(),
            &trust,
            &cancel,
            Duration::from_secs(1)
        )
        .unwrap_err(),
        Failure::Cancelled
    );
}
