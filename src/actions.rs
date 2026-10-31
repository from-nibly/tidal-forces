use crate::links::Link;
use anyhow::{Result, bail, ensure};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
    Seek(Duration),
    SeekBy(i64),
    Volume(f32),
    VolumeBy(f32),
    NowPlaying,
    MiniPlayer,
    Open(Link),
    Raise,
    Quit,
}

impl Action {
    pub fn media(event: crate::desktop::MediaControlEvent) -> Result<Option<Self>> {
        use crate::desktop::MediaControlEvent as Event;
        let relative = |direction, duration: Duration| {
            let millis = duration.as_millis().min(i64::MAX as u128) as i64;
            Self::SeekBy(if direction == souvlaki::SeekDirection::Forward {
                millis
            } else {
                -millis
            })
        };
        Ok(Some(match event {
            Event::Play => Self::Play,
            Event::Pause => Self::Pause,
            Event::Toggle => Self::Toggle,
            Event::Next => Self::Next,
            Event::Previous => Self::Previous,
            Event::Stop => Self::Stop,
            Event::SetPosition(position) => Self::Seek(position.0),
            Event::SeekBy(direction, duration) => relative(direction, duration),
            Event::Seek(direction) => relative(direction, Duration::from_secs(10)),
            Event::SetVolume(value) if value.is_finite() => {
                Self::Volume(value.clamp(0., 1.) as f32)
            }
            Event::OpenUri(uri) => Self::Open(Link::parse(&uri)?),
            Event::Raise => Self::Raise,
            Event::Quit => Self::Quit,
            _ => return Ok(None),
        }))
    }

    pub fn parse(command: &str, value: &str) -> Result<Self> {
        ensure!(value.len() <= 32, "Command argument is too long");
        let number = || -> Result<f64> {
            let number: f64 = value
                .parse()
                .map_err(|_| anyhow::anyhow!("Expected a finite number"))?;
            ensure!(number.is_finite(), "Expected a finite number");
            Ok(number)
        };
        match command {
            "seek" | "seek-by" => {
                let seconds = number()?;
                ensure!(
                    seconds.abs() <= 31. * 86_400.,
                    "Seek exceeds the 31-day limit"
                );
                let millis = (seconds * 1000.).round() as i64;
                if command == "seek" {
                    ensure!(seconds >= 0., "Seek position must be nonnegative");
                    Ok(Self::Seek(Duration::from_millis(millis as u64)))
                } else {
                    Ok(Self::SeekBy(millis))
                }
            }
            "volume" => {
                let percent = number()?;
                ensure!(
                    (0. ..=100.).contains(&percent),
                    "Volume must be between 0 and 100 percent"
                );
                Ok(Self::Volume((percent / 100.) as f32))
            }
            _ => {
                ensure!(value.is_empty(), "This command takes no argument");
                Ok(match command {
                    "play" => Self::Play,
                    "pause" => Self::Pause,
                    "toggle" => Self::Toggle,
                    "next" => Self::Next,
                    "previous" => Self::Previous,
                    "stop" => Self::Stop,
                    "raise" => Self::Raise,
                    "now-playing" => Self::NowPlaying,
                    "mini-player" => Self::MiniPlayer,
                    "quit" => Self::Quit,
                    _ => bail!("Unknown player command"),
                })
            }
        }
    }
}

pub fn cli(args: &[String]) -> Result<Option<(&str, &str)>> {
    let Some(command) = args.first().and_then(|s| s.strip_prefix("--")) else {
        return Ok(None);
    };
    let takes_value = matches!(command, "seek" | "seek-by" | "volume");
    if !takes_value
        && !matches!(
            command,
            "play"
                | "pause"
                | "toggle"
                | "next"
                | "previous"
                | "stop"
                | "raise"
                | "quit"
                | "now-playing"
                | "mini-player"
        )
    {
        return Ok(None);
    }
    ensure!(
        args.len() == if takes_value { 2 } else { 1 },
        "Incorrect number of player command arguments"
    );
    let value = args.get(1).map_or("", String::as_str);
    Action::parse(command, value)?;
    Ok(Some((command, value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_bounded_and_preserve_fractional_seconds() {
        assert_eq!(
            Action::parse("seek", "12.345").unwrap(),
            Action::Seek(Duration::from_millis(12_345))
        );
        assert_eq!(
            Action::parse("seek-by", "-0.5").unwrap(),
            Action::SeekBy(-500)
        );
        assert_eq!(Action::parse("volume", "25").unwrap(), Action::Volume(0.25));
        for (command, value) in [
            ("seek", "-1"),
            ("seek", "NaN"),
            ("seek-by", "inf"),
            ("seek", "2678401"),
            ("volume", "101"),
            ("volume", "-1"),
            ("play", "private"),
            ("unknown", ""),
        ] {
            let error = Action::parse(command, value).unwrap_err().to_string();
            assert!(!error.contains("private"));
        }
    }

    #[test]
    fn cli_controls_require_exact_argument_counts() {
        let args = |values: &[&str]| values.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(cli(&args(&["--play"])).unwrap(), Some(("play", "")));
        assert!(cli(&args(&["--seek"])).is_err());
        assert!(cli(&args(&["--pause", "extra"])).is_err());
        assert!(cli(&args(&["--volume", "20", "extra"])).is_err());
        assert!(
            cli(&args(&["--open", "tidal://track/1"]))
                .unwrap()
                .is_none()
        );
    }
}
