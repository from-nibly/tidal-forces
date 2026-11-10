use super::*;
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{ring::default_provider, verify_tls12_signature, verify_tls13_signature},
};
use rustls_pki_types::ServerName;
use std::{
    io::{self, Read, Write},
    net::{IpAddr, SocketAddr, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

const CAST: &[u8] = include_bytes!("roots/cast.der");
const EUREKA: &[u8] = include_bytes!("roots/eureka.der");
const CRL: &[u8] = include_bytes!("roots/crl.der");
const MAX_WIRE: usize = 512 * 1024;
#[path = "tone.rs"]
mod tone;

fn official_trust() -> Trust<'static> {
    Trust {
        device: &[CAST, EUREKA],
        revocation: &[CRL],
        allow_legacy_device_chain: false,
    }
}

// Cast authenticates its short-lived TLS certificate through deviceauth, not
// public hostname PKI. This verifier checks TLS key possession and certificate
// shape/time only. The connection stays private/quarantined until Context succeeds;
// no caller gets an untrusted stream. Only one AuthChallenge precedes trust;
// the separately opted-in tone test runs only after complete authentication.
#[derive(Debug)]
struct QuarantinedPeer;
impl ServerCertVerifier for QuarantinedPeer {
    fn verify_server_cert(
        &self,
        end: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let invalid = || rustls::Error::General("Cast quarantine certificate rejected".into());
        if intermediates.len() > 4
            || intermediates.iter().any(|c| c.len() > MAX_CERT)
            || ocsp.len() > MAX_CERT
        {
            return Err(invalid());
        }
        let cert = certificate(end.as_ref()).map_err(|_| invalid())?;
        let end_time = expiry(&cert, now.as_secs()).map_err(|_| invalid())?;
        if end_time.saturating_sub(now.as_secs()) > 4 * 86400 {
            return Err(invalid());
        }
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        msg: &[u8],
        cert: &CertificateDer<'_>,
        sig: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            msg,
            cert,
            sig,
            &default_provider().signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        msg: &[u8],
        cert: &CertificateDer<'_>,
        sig: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            msg,
            cert,
            sig,
            &default_provider().signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

struct BudgetIo<'a> {
    socket: TcpStream,
    deadline: Instant,
    cancel: &'a AtomicBool,
    transferred: usize,
    wire_limit: usize,
    valid_until: Option<u64>,
}
impl BudgetIo<'_> {
    fn check(&self) -> io::Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::ConnectionAborted.into());
        }
        if Instant::now() >= self.deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        if self
            .valid_until
            .is_some_and(|end| clock().map_or(true, |now| now >= end))
        {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        if self.transferred >= self.wire_limit {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(())
    }
    fn window(&self) -> Duration {
        self.deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(100))
    }
}
impl Read for BudgetIo<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        loop {
            self.check()?;
            let count = out.len().min(self.wire_limit - self.transferred);
            self.socket
                .set_read_timeout(Some(self.window().max(Duration::from_micros(1))))?;
            match self.socket.read(&mut out[..count]) {
                Ok(n) => {
                    self.transferred += n;
                    return Ok(n);
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::TimedOut
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::Interrupted
                    ) =>
                {
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }
}
impl Write for BudgetIo<'_> {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if input.is_empty() {
            return Ok(0);
        }
        loop {
            self.check()?;
            let count = input.len().min(self.wire_limit - self.transferred);
            self.socket
                .set_write_timeout(Some(self.window().max(Duration::from_micros(1))))?;
            match self.socket.write(&input[..count]) {
                Ok(n) => {
                    self.transferred += n;
                    return Ok(n);
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::TimedOut
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::Interrupted
                    ) =>
                {
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        self.check()?;
        self.socket.flush()
    }
}

fn clock() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| Failure::TlsValidity)
}

fn authenticate_target(
    address: SocketAddr,
    trust: &Trust<'_>,
    cancel: &AtomicBool,
    budget: Duration,
) -> Result<Authenticated> {
    run_target(address, trust, cancel, budget, None)
}

fn run_target(
    address: SocketAddr,
    trust: &Trust<'_>,
    cancel: &AtomicBool,
    budget: Duration,
    fixture: Option<tone::Fixture>,
) -> Result<Authenticated> {
    let sample = fixture.map(tone::sample);
    run_sample_target(address, trust, cancel, budget, sample.as_ref(), &mut || {
        Ok(())
    })
}

fn run_sample_target(
    address: SocketAddr,
    trust: &Trust<'_>,
    cancel: &AtomicBool,
    budget: Duration,
    sample: Option<&tone::Sample>,
    guard: &mut dyn FnMut() -> std::result::Result<(), &'static str>,
) -> Result<Authenticated> {
    run_authenticated(address, trust, cancel, budget, sample, guard, None)
}

enum Runtime<'a> {
    Play {
        media: &'a crate::cast_control::Media,
        control: &'a crate::cast_control::Control,
        released: &'a mut bool,
        ended: &'a mut Option<crate::cast_control::End>,
    },
    Idle(&'a mut bool),
}
fn run_authenticated(
    address: SocketAddr,
    trust: &Trust<'_>,
    cancel: &AtomicBool,
    budget: Duration,
    sample: Option<&tone::Sample>,
    guard: &mut dyn FnMut() -> std::result::Result<(), &'static str>,
    runtime: Option<Runtime<'_>>,
) -> Result<Authenticated> {
    guard().map_err(Failure::Media)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(Failure::Cancelled);
    }
    if budget.is_zero() || budget > Duration::from_secs(10) {
        return Err(Failure::Bounds);
    }
    let deadline = Instant::now() + budget;
    let socket = TcpStream::connect_timeout(&address, budget.min(Duration::from_secs(2)))
        .map_err(|_| Failure::Transport)?;
    let io = BudgetIo {
        socket,
        deadline,
        cancel,
        transferred: 0,
        wire_limit: MAX_WIRE,
        valid_until: None,
    };
    let mut config = ClientConfig::builder_with_provider(Arc::new(default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|_| Failure::Transport)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(QuarantinedPeer))
        .with_no_client_auth();
    config.resumption = rustls::client::Resumption::disabled();
    config.enable_sni = false;
    config.enable_early_data = false;
    config.key_log = Arc::new(rustls::NoKeyLog {});
    let mut connection =
        ClientConnection::new(Arc::new(config), ServerName::IpAddress(address.ip().into()))
            .map_err(|_| Failure::Transport)?;
    connection.set_buffer_limit(Some(MAX_MESSAGE));
    let mut stream = StreamOwned::new(connection, io);
    let operation = (|| {
        while stream.conn.is_handshaking() {
            stream
                .conn
                .complete_io(&mut stream.sock)
                .map_err(|_| Failure::Transport)?;
        }
        let peer = stream
            .conn
            .peer_certificates()
            .and_then(|chain| chain.first())
            .ok_or(Failure::TlsValidity)?
            .to_vec();
        let context = Context::new(&peer)?;
        let challenge = context.challenge();
        stream
            .write_all(&(challenge.len() as u32).to_be_bytes())
            .map_err(|_| Failure::Transport)?;
        stream
            .write_all(&challenge)
            .map_err(|_| Failure::Transport)?;
        stream.flush().map_err(|_| Failure::Transport)?;
        let mut header = [0; 4];
        stream
            .read_exact(&mut header)
            .map_err(|_| Failure::Transport)?;
        let count = u32::from_be_bytes(header) as usize;
        if count == 0 || count > MAX_MESSAGE {
            return Err(Failure::Bounds);
        }
        let mut bytes = vec![0; count];
        stream
            .read_exact(&mut bytes)
            .map_err(|_| Failure::Transport)?;
        let proof = context.authenticate(&bytes, trust, clock()?)?;
        if proof.peer_fingerprint != <[u8; 32]>::from(Sha256::digest(&peer)) {
            return Err(Failure::Signature);
        }
        match runtime {
            Some(Runtime::Play {
                media,
                control,
                released,
                ended,
            }) => {
                let window = media.duration.saturating_mul(3) + Duration::from_secs(120);
                if media.duration.is_zero()
                    || media.duration > Duration::from_secs(900)
                    || proof.valid_until <= clock()?.saturating_add(window.as_secs() + 10)
                {
                    return Err(Failure::Deadline);
                }
                stream.sock.valid_until = Some(proof.valid_until);
                stream.sock.deadline = Instant::now() + window;
                stream.sock.wire_limit = 128 * 1024 * 1024;
                *ended = Some(
                    tone::run_runtime(&mut stream, media, control, guard, released)
                        .map_err(Failure::Media)?,
                );
            }
            Some(Runtime::Idle(result)) => {
                if proof.valid_until <= clock()?.saturating_add(20) {
                    return Err(Failure::Deadline);
                }
                stream.sock.valid_until = Some(proof.valid_until);
                stream.sock.deadline = Instant::now() + Duration::from_secs(8);
                *result = tone::inspect_idle(&mut stream).map_err(Failure::Media)?;
            }
            None => {}
        }
        if let Some(sample) = sample {
            let window = sample
                .full_duration
                .map_or(Duration::from_secs(20), |d| d + Duration::from_secs(30));
            if proof.valid_until <= clock()?.saturating_add(window.as_secs() + 10) {
                return Err(Failure::Deadline);
            }
            stream.sock.valid_until = Some(proof.valid_until);
            stream.sock.deadline = deadline + window;
            if sample.full_duration.is_some() {
                stream.sock.wire_limit = 32 * 1024 * 1024;
            }
            println!("Device authentication passed; checking receiver idle state.");
            tone::run_sample(&mut stream, sample, guard)?;
        }
        Ok(proof)
    })();
    // Drop/shutdown on every outcome; never return a reusable untrusted stream.
    let _ = stream.sock.socket.shutdown(std::net::Shutdown::Both);
    if cancel.load(Ordering::Relaxed) {
        return Err(Failure::Cancelled);
    }
    if Instant::now() >= stream.sock.deadline {
        return Err(Failure::Deadline);
    }
    operation
}

pub(crate) fn verify_idle(
    endpoint: std::net::SocketAddrV4,
    legacy: bool,
) -> std::result::Result<bool, &'static str> {
    let ip = endpoint.ip();
    if endpoint.port() == 0
        || !ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
    {
        return Err("Invalid selected receiver address");
    }
    let mut trust = official_trust();
    trust.allow_legacy_device_chain = legacy;
    let mut idle = false;
    run_authenticated(
        endpoint.into(),
        &trust,
        &AtomicBool::new(false),
        Duration::from_secs(10),
        None,
        &mut || Ok(()),
        Some(Runtime::Idle(&mut idle)),
    )
    .map_err(|_| "Could not authenticate and verify receiver idle state")?;
    Ok(idle)
}

pub(crate) fn run_runtime(
    endpoint: std::net::SocketAddrV4,
    legacy: bool,
    media: crate::cast_control::Media,
    control: crate::cast_control::Control,
    guard: &mut dyn FnMut() -> std::result::Result<(), &'static str>,
) -> (
    std::result::Result<crate::cast_control::End, &'static str>,
    bool,
) {
    let ip = endpoint.ip();
    if endpoint.port() == 0
        || !ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
    {
        let result = Err("Invalid explicitly selected receiver address");
        control.close(result, true);
        return (result, true);
    }
    let mut trust = official_trust();
    trust.allow_legacy_device_chain = legacy;
    let mut released = true;
    let mut ended = None;
    if control
        .emit(crate::cast_control::Event::Phase(
            crate::cast_control::Phase::Authenticating,
        ))
        .is_err()
    {
        let result = Err("Cast controller is unavailable");
        control.close(result, true);
        return (result, true);
    }
    let result = run_authenticated(
        endpoint.into(),
        &trust,
        &AtomicBool::new(false),
        Duration::from_secs(10),
        None,
        guard,
        Some(Runtime::Play {
            media: &media,
            control: &control,
            released: &mut released,
            ended: &mut ended,
        }),
    )
    .map(|_| ended.unwrap_or(crate::cast_control::End::Stopped))
    .map_err(|failure| match failure {
        Failure::Media(reason) | Failure::CertificateAlgorithm(reason) => reason,
        _ => "Cast connection or authentication failed; no automatic retry",
    });
    // Transport has shut down before the UI is allowed to start a replacement.
    control.close(result, released);
    (result, released)
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn current_track(
    ip: std::net::Ipv4Addr,
    xml: &str,
    full_duration: Option<Duration>,
    guard: &mut dyn FnMut() -> std::result::Result<(), &'static str>,
) -> std::result::Result<(), &'static str> {
    if !ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_broadcast() {
        return Err("Invalid explicitly selected receiver address");
    }
    let mut trust = official_trust();
    trust.allow_legacy_device_chain = true;
    let sample = tone::current_track_sample(xml, full_duration)?;
    run_sample_target(
        SocketAddr::new(ip.into(), 8009),
        &trust,
        &AtomicBool::new(false),
        Duration::from_secs(10),
        Some(&sample),
        guard,
    )
    .map(|_| ())
    .map_err(|failure| match failure {
        Failure::Media(reason) | Failure::CertificateAlgorithm(reason) => reason,
        _ => {
            "Account-media probe stopped; no automatic retry. Check receiver if controls had begun"
        }
    })
}

pub(super) fn inspect_ip(
    ip: IpAddr,
    allow_legacy: bool,
) -> std::result::Result<(bool, u64), &'static str> {
    let IpAddr::V4(ipv4) = ip else {
        return Err("Only an explicit private IPv4 target is supported");
    };
    if !ipv4.is_private() || ipv4.is_loopback() || ipv4.is_link_local() || ipv4.is_broadcast() {
        return Err("Target is not an allowed private IPv4 address");
    }
    let cancelled = AtomicBool::new(false);
    let mut trust = official_trust();
    trust.allow_legacy_device_chain = allow_legacy;
    authenticate_target(
        SocketAddr::new(ip, 8009),
        &trust,
        &cancelled,
        Duration::from_secs(10),
    )
    .map(|p| (p.audio_only, p.valid_until))
    .map_err(|error| match error {
        Failure::Chain => "Device certificate chain rejected",
        Failure::CertificateAlgorithm(reason) => reason,
        Failure::Revocation => "Mandatory signed revocation proof rejected",
        Failure::Signature => "Device signature or TLS binding rejected",
        Failure::Nonce => "Device challenge nonce rejected",
        Failure::Algorithm => "Device authentication algorithm rejected",
        Failure::Deadline => "Authentication deadline exceeded",
        Failure::Bounds => "Authentication size limits exceeded",
        Failure::Envelope => "Unexpected authentication envelope",
        Failure::Transport => "TLS handshake or authentication IO failed",
        _ => "Authentication rejected",
    })
}

fn parse_mode(args: &[String]) -> std::result::Result<bool, &'static str> {
    match args {
        [base] if base == "--authenticate-ip-stdin" => Ok(false),
        [base, legacy]
            if base == "--authenticate-ip-stdin"
                && legacy == "--allow-legacy-device-certificates" =>
        {
            Ok(true)
        }
        _ => Err(
            "Explicit --authenticate-ip-stdin required; only optional --allow-legacy-device-certificates is supported",
        ),
    }
}

fn tone_mode(args: &[String]) -> bool {
    args == [
        "--tone-test-ip-stdin",
        "--allow-legacy-device-certificates",
        "--volume-percent=100",
    ]
}

fn dash_legacy_mode(args: &[String]) -> bool {
    matches!(args, [mode, legacy, volume]
        if mode == "--dash-data-url-test-ip-stdin"
            && legacy == "--allow-legacy-device-certificates"
            && matches!(volume.as_str(), "--volume-percent=50" | "--volume-percent=100"))
}
fn dash_mode(args: &[String]) -> bool {
    matches!(args, [mode, volume]
        if mode == "--dash-data-url-test-ip-stdin"
            && matches!(volume.as_str(), "--volume-percent=50" | "--volume-percent=100"))
        || dash_legacy_mode(args)
}
fn dash_full_mode(args: &[String]) -> bool {
    dash_mode(args) && args.last().is_some_and(|a| a == "--volume-percent=100")
}

pub(crate) fn inspect() -> std::result::Result<(), &'static str> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let tone_test = tone_mode(&args);
    let dash_test = dash_mode(&args);
    let allow_legacy = if tone_test || dash_legacy_mode(&args) {
        true
    } else if dash_test {
        false
    } else {
        parse_mode(&args)?
    };
    let mut input = String::new();
    std::io::stdin()
        .take(65)
        .read_to_string(&mut input)
        .map_err(|_| "Invalid target input")?;
    if input.len() > 64 {
        return Err("Target input too long");
    }
    let ip = input
        .trim()
        .parse()
        .map_err(|_| "Expected one IP address")?;
    if tone_test || dash_test {
        let IpAddr::V4(v4) = ip else {
            return Err("Only private IPv4 targets are supported");
        };
        if !v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_broadcast() {
            return Err("Target is not an allowed private IPv4 address");
        }
        let mut trust = official_trust();
        trust.allow_legacy_device_chain = allow_legacy;
        let fixture = if dash_full_mode(&args) {
            tone::Fixture::InlineDashFullVolume
        } else if dash_test {
            tone::Fixture::InlineDash
        } else {
            tone::Fixture::Tone
        };
        return run_target(SocketAddr::new(ip,8009), &trust, &AtomicBool::new(false), Duration::from_secs(10), Some(fixture))
            .map(|_| ())
            .map_err(|error| match error { Failure::Media(reason) => reason, Failure::Transport => "Connection, TLS handshake or authentication IO failed; no automatic retry", Failure::CertificateAlgorithm(reason) => reason, _ => "Tone probe failed; do not retry blindly. Check receiver state if controls had begun" });
    }
    let (audio_only, valid_until) = inspect_ip(ip, allow_legacy)?;
    println!(
        "Authenticated receiver: audio_only={audio_only}; legacy_device_chain={allow_legacy}; proof_valid_until={valid_until}. No application commands sent."
    );
    Ok(())
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
