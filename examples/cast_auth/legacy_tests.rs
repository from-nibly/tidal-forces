//! Private legacy-policy regression checks; no receiver contact.
//! The shared adapter is enabled in the probe only through explicit CLI opt-in.
use super::legacy_certificate::LegacyCertificateRsaSha1;
use super::*;
fn legacy_path(leaf: &[u8], intermediate: &[u8], root: &[u8], now: u64) -> bool {
    let leaf = CertificateDer::from(leaf);
    let Ok(end) = EndEntityCert::try_from(&leaf) else {
        return false;
    };
    let root = CertificateDer::from(root);
    let Ok(anchor) = anchor_from_trusted_cert(&root) else {
        return false;
    };
    let anchors = [anchor];
    let intermediates = [CertificateDer::from(intermediate)];
    end.verify_for_usage(
        &[RSA_SHA256, &LegacyCertificateRsaSha1],
        &anchors,
        &intermediates,
        UnixTime::since_unix_epoch(Duration::from_secs(now)),
        KeyUsage::client_auth(),
        None,
        None,
    )
    .is_ok()
}

#[test]
#[ignore = "requires tests/cast-auth.py ephemeral synthetic fixtures; offline research only"]
fn private_legacy_contract() {
    let root = std::path::PathBuf::from(
        std::env::var_os("TIDAL_CAST_PRIVATE_FIXTURES").expect("private runner required"),
    );
    let read = |name: &str| std::fs::read(root.join("fixtures").join(name)).unwrap();
    let leaf = read("sha1-device.der");
    let intermediate = read("intermediate.der");
    let device_root = read("root.der");
    let now: u64 = std::str::from_utf8(&read("clock.txt"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(legacy_path(&leaf, &intermediate, &device_root, now));
    assert!(!legacy_path(
        &leaf,
        &intermediate,
        &read("untrusted-root.der"),
        now
    ));
    let mut altered = leaf.clone();
    let last = altered.len() - 1;
    altered[last] ^= 1;
    assert!(!legacy_path(&altered, &intermediate, &device_root, now));
    assert!(!legacy_path(
        &leaf,
        &intermediate,
        &device_root,
        now + 366 * 86400
    ));
    let strict = verify_chain(
        &leaf,
        std::slice::from_ref(&intermediate),
        &[device_root.as_slice()],
        now,
    );
    assert_eq!(
        strict.unwrap_err(),
        Failure::CertificateAlgorithm("Rejected certificate algorithm: RSA/SHA-1")
    );

    let envelope = proto::Envelope::decode(read("sha1-device.reply").as_slice()).unwrap();
    let auth = proto::Auth::decode(envelope.binary.as_deref().unwrap()).unwrap();
    let response = auth.response.unwrap();
    assert_eq!(response.hash_algorithm, Some(1));
    let mut binding = response.nonce.clone().unwrap();
    binding.extend_from_slice(&read("tls.der"));
    let signature = response.signature.unwrap();
    verify_signature(&leaf, &binding, &signature).unwrap();
    binding[0] ^= 1;
    assert_eq!(
        verify_signature(&leaf, &binding, &signature).unwrap_err(),
        Failure::Signature
    );

    let device_roots = [device_root.as_slice()];
    let crl_root = read("crl-root.der");
    let crl_roots = [crl_root.as_slice()];
    let trust = Trust {
        device: &device_roots,
        revocation: &crl_roots,
        allow_legacy_device_chain: false,
    };
    let chain = vec![leaf, intermediate, device_root.clone()];
    let clean = verify_crl(response.crl.as_deref().unwrap(), &trust, now).unwrap();
    check_revocation(&chain, &clean).unwrap();
    // The legacy and modern synthetic leaf certificates share the same key;
    // revoking that key must reject both, regardless of certificate digest.
    let revoked = verify_crl(&read("revoked-device.crl"), &trust, now).unwrap();
    assert_eq!(
        check_revocation(&chain, &revoked).unwrap_err(),
        Failure::Revocation
    );
    let bad = Trust {
        device: &device_roots,
        revocation: &device_roots,
        allow_legacy_device_chain: false,
    };
    assert_eq!(
        verify_crl(response.crl.as_deref().unwrap(), &bad, now).unwrap_err(),
        Failure::Revocation
    );
    assert_eq!(
        verify_crl(&read("expired.crl"), &trust, now).unwrap_err(),
        Failure::Revocation
    );
    let mut env = proto::Envelope::decode(read("sha1-device.reply").as_slice()).unwrap();
    let mut auth = proto::Auth::decode(env.binary.as_deref().unwrap()).unwrap();
    let response = auth.response.as_mut().unwrap();
    response.hash_algorithm = Some(0);
    let mut context = Context::new(&read("tls.der")).unwrap();
    context
        .nonce
        .copy_from_slice(response.nonce.as_deref().unwrap());
    env.binary = Some(auth.encode_to_vec());
    assert_eq!(
        context
            .authenticate(&env.encode_to_vec(), &trust, now)
            .unwrap_err(),
        Failure::Algorithm
    );
    println!("Offline legacy-chain checks passed; strict policy remains the default.");
}
