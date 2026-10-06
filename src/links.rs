use anyhow::{Result, bail, ensure};
use std::sync::{Arc, Mutex, mpsc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    Track(u64),
    Album(u64),
    Artist(u64),
    Playlist(String),
    Mix(String),
}

impl Link {
    pub fn parse(value: &str) -> Result<Self> {
        ensure!(
            value.len() <= 4096 && !value.bytes().any(|b| b <= 32),
            "Invalid TIDAL link"
        );
        let url = reqwest::Url::parse(value).map_err(|_| anyhow::anyhow!("Invalid TIDAL link"))?;
        ensure!(
            url.username().is_empty() && url.password().is_none() && url.port().is_none(),
            "Invalid TIDAL link authority"
        );
        let mut parts: Vec<&str> = url.path().trim_matches('/').split('/').collect();
        match url.scheme() {
            "tidal" => {
                if let Some(host) = url.host_str() {
                    parts.insert(0, host);
                }
            }
            "https" => ensure!(
                matches!(
                    url.host_str(),
                    Some("tidal.com" | "www.tidal.com" | "listen.tidal.com")
                ),
                "Not a TIDAL link"
            ),
            _ => bail!("Only tidal:// and official TIDAL HTTPS links are supported"),
        }
        if parts.first() == Some(&"browse") {
            parts.remove(0);
        }
        ensure!(
            parts.len() == 2,
            "Expected a TIDAL track, album, artist, playlist or mix link"
        );
        let id = parts[1];
        let numeric = || -> Result<u64> {
            ensure!(
                !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()),
                "Invalid TIDAL ID"
            );
            let number = id.parse::<u64>()?;
            ensure!(number > 0, "Invalid TIDAL ID");
            Ok(number)
        };
        Ok(match parts[0] {
            "track" => Self::Track(numeric()?),
            "album" => Self::Album(numeric()?),
            "artist" => Self::Artist(numeric()?),
            "playlist" => {
                ensure!(
                    id.len() == 36
                        && id
                            .bytes()
                            .enumerate()
                            .all(|(i, b)| if [8, 13, 18, 23].contains(&i) {
                                b == b'-'
                            } else {
                                b.is_ascii_hexdigit()
                            }),
                    "Invalid playlist UUID"
                );
                Self::Playlist(id.to_owned())
            }
            "mix" => {
                ensure!(
                    !id.is_empty()
                        && id.len() <= 200
                        && id
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
                    "Invalid mix ID"
                );
                Self::Mix(id.to_owned())
            }
            _ => bail!("Unsupported TIDAL link type"),
        })
    }
}

const SERVICE: &str = "rocks.tidalforces.Player";
const PATH: &str = "/rocks/tidalforces/Player";
type Wake = Arc<Mutex<Option<eframe::egui::Context>>>;

struct Inbox {
    tx: mpsc::SyncSender<Option<Link>>,
    wake: Wake,
}
#[zbus::dbus_interface(name = "rocks.tidalforces.Player")]
impl Inbox {
    fn open(&self, uri: &str) -> zbus::fdo::Result<()> {
        let link = if uri.is_empty() {
            None
        } else {
            Some(Link::parse(uri).map_err(|e| zbus::fdo::Error::InvalidArgs(e.to_string()))?)
        };
        self.tx.try_send(link).map_err(|_| {
            zbus::fdo::Error::LimitsExceeded("The player is busy; try again".into())
        })?;
        if let Some(ctx) = self.wake.lock().unwrap().as_ref() {
            ctx.request_repaint();
        }
        Ok(())
    }
}

pub struct Instance {
    // Holding the bus connection owns the name, including while the GUI starts.
    _connection: zbus::blocking::Connection,
    pub events: mpsc::Receiver<Option<Link>>,
    wake: Wake,
}
impl Instance {
    // None means a running player accepted the activation/link.
    pub fn start(uri: Option<&str>) -> Result<Option<Self>> {
        if let Some(uri) = uri {
            Link::parse(uri)?;
        }
        let (tx, events) = mpsc::sync_channel(32);
        let wake = Arc::new(Mutex::new(None));
        let inbox = Inbox {
            tx,
            wake: wake.clone(),
        };
        let connection = zbus::blocking::ConnectionBuilder::session()?
            .serve_at(PATH, inbox)?
            .name(SERVICE)?
            .build();
        match connection {
            Ok(connection) => Ok(Some(Self {
                _connection: connection,
                events,
                wake,
            })),
            Err(zbus::Error::NameTaken) => {
                let connection = zbus::blocking::Connection::session()?;
                let proxy = zbus::blocking::Proxy::new(&connection, SERVICE, PATH, SERVICE)?;
                proxy.call::<_, _, ()>("Open", &(uri.unwrap_or(""),))?;
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }
    pub fn attach(&self, ctx: eframe::egui::Context) {
        *self.wake.lock().unwrap() = Some(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Run under dbus-run-session so the test does not contact your real player"]
    fn single_instance_forwards_links_and_activation() {
        let primary = Instance::start(None).unwrap().unwrap();
        primary.attach(eframe::egui::Context::default());
        assert!(
            Instance::start(Some("tidal://track/123"))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            primary
                .events
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            Some(Link::Track(123))
        );
        assert!(Instance::start(None).unwrap().is_none());
        assert_eq!(
            primary
                .events
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            None
        );
        assert!(Instance::start(Some("tidal://login/auth?code=secret")).is_err());
        assert!(primary.events.try_recv().is_err());
    }

    #[test]
    fn parses_tidal_and_official_web_links() {
        for uri in [
            "tidal://track/123",
            "tidal://browse/track/123",
            "tidal:///track/123",
            "https://tidal.com/browse/track/123?u=share",
            "https://listen.tidal.com/track/123",
        ] {
            assert_eq!(Link::parse(uri).unwrap(), Link::Track(123));
        }
        assert_eq!(Link::parse("tidal://album/12").unwrap(), Link::Album(12));
        assert_eq!(Link::parse("tidal://artist/12").unwrap(), Link::Artist(12));
        assert_eq!(
            Link::parse("tidal://mix/abc_123").unwrap(),
            Link::Mix("abc_123".into())
        );
        assert!(matches!(
            Link::parse("tidal://playlist/12345678-1234-1234-1234-123456789abc").unwrap(),
            Link::Playlist(_)
        ));
    }
    #[test]
    fn rejects_untrusted_and_malformed_links() {
        for uri in [
            "https://tidal.com.evil.test/track/1",
            "https://evil.test/track/1",
            "http://tidal.com/track/1",
            "file:///track/1",
            "tidal://user@track/1",
            "tidal://track:99/1",
            "tidal://track/0",
            "tidal://track/1/extra",
            "tidal://track/%31",
            "tidal://track/+1",
            "tidal://playlist/not-a-uuid",
            "tidal://mix/a%2Fb",
            "tidal://login/auth?code=secret",
            "tidal://track/1\n",
        ] {
            assert!(Link::parse(uri).is_err(), "{uri}");
        }
    }
}
