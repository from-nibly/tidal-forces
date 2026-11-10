//! Test-only, finite clear-DASH excerpt; never persists or logs signed URLs.
#[cfg(test)]
use crate::dash::Codec;
use crate::dash::Manifest;
use anyhow::{Result, ensure};
#[cfg(test)]
use std::fmt::Write;
use std::time::Duration;

pub(super) struct Clip {
    pub xml: String,
    pub duration: Duration,
}
fn url(value: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(value).map_err(|_| anyhow::anyhow!("Invalid media URL"))?;
    let host = parsed.host_str().unwrap_or_default();
    ensure!(
        value.len() <= 4096
            && parsed.scheme() == "https"
            && (host == "audio.tidal.com" || host.ends_with(".audio.tidal.com"))
            && parsed.port_or_known_default() == Some(443)
            && parsed.username().is_empty()
            && parsed.password().is_none()
            && parsed.fragment().is_none(),
        "Media URL is outside the reviewed TIDAL audio-CDN scope"
    );
    Ok(())
}
#[cfg(test)]
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
#[cfg(test)]
pub(super) fn build(manifest: &Manifest) -> Result<Clip> {
    url(&manifest.init)?;
    ensure!(
        (8000..=96000).contains(&manifest.sample_rate),
        "Sample rate outside this receiver test's scope; no downgrade"
    );
    let mut duration = Duration::ZERO;
    let mut selected = Vec::new();
    for segment in &manifest.segments {
        ensure!(
            !segment.duration.is_zero()
                && segment.start.abs_diff(duration) <= Duration::from_micros(1),
            "Non-contiguous excerpt rejected"
        );
        let end = duration
            .checked_add(segment.duration)
            .ok_or_else(|| anyhow::anyhow!("Excerpt duration overflow"))?;
        if end > Duration::from_secs(8) || selected.len() == 8 {
            break;
        }
        url(&segment.url)?;
        selected.push(segment);
        duration = end;
    }
    ensure!(
        !selected.is_empty(),
        "No complete media segment fits the eight-second excerpt; no full-track fallback"
    );
    let codec = match manifest.codec {
        Codec::Flac => "flac",
        Codec::AacLc => "mp4a.40.2",
        Codec::HeAac => "mp4a.40.5",
        Codec::HeAacV2 => "mp4a.40.29",
    };
    let mut xml = format!(
        "<MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\" type=\"static\" profiles=\"urn:mpeg:dash:profile:isoff-main:2011\" mediaPresentationDuration=\"PT{:.9}S\" minBufferTime=\"PT1S\"><Period start=\"PT0S\" duration=\"PT{:.9}S\"><AdaptationSet contentType=\"audio\" mimeType=\"audio/mp4\"><Representation id=\"1\" codecs=\"{codec}\" audioSamplingRate=\"{}\" bandwidth=\"10000000\"><SegmentList timescale=\"1000000000\"><Initialization sourceURL=\"{}\"/><SegmentTimeline>",
        duration.as_secs_f64(),
        duration.as_secs_f64(),
        manifest.sample_rate,
        escape(&manifest.init)
    );
    for segment in &selected {
        write!(xml, "<S d=\"{}\"/>", segment.duration.as_nanos())?;
    }
    xml.push_str("</SegmentTimeline>");
    for segment in &selected {
        write!(xml, "<SegmentURL media=\"{}\"/>", escape(&segment.url))?;
    }
    xml.push_str("</SegmentList></Representation></AdaptationSet></Period></MPD>");
    ensure!(
        xml.len() <= 8192,
        "Excerpt exceeds private transport bounds"
    );
    Ok(Clip { xml, duration })
}

#[cfg(test)]
pub(super) fn structure(xml: &str) -> Result<String> {
    let document = roxmltree::Document::parse(xml)
        .map_err(|_| anyhow::anyhow!("Invalid manifest structure"))?;
    let namespace = |value: Option<&str>| match value {
        None => "absent",
        Some("urn:mpeg:dash:schema:mpd:2011") => "standard DASH",
        Some(value) if value.eq_ignore_ascii_case("urn:mpeg:dash:schema:mpd:2011") => {
            "DASH case variant"
        }
        _ => "other (redacted)",
    };
    let known_tags = [
        "MPD",
        "Period",
        "AdaptationSet",
        "Representation",
        "AudioChannelConfiguration",
        "SegmentTemplate",
        "SegmentTimeline",
        "S",
        "Role",
        "Label",
        "BaseURL",
        "Location",
        "UTCTiming",
        "EssentialProperty",
        "SupplementalProperty",
        "ContentProtection",
        "ProgramInformation",
        "Title",
        "Source",
        "Copyright",
        "ServiceDescription",
        "ProducerReferenceTime",
        "EventStream",
        "Event",
        "SegmentList",
        "SegmentURL",
        "Initialization",
        "SubRepresentation",
        "ContentComponent",
    ];
    let known_attributes = [
        "type",
        "id",
        "codecs",
        "bandwidth",
        "audioSamplingRate",
        "contentType",
        "mimeType",
        "segmentAlignment",
        "startWithSAP",
        "mediaPresentationDuration",
        "minBufferTime",
        "profiles",
        "start",
        "duration",
        "timescale",
        "startNumber",
        "initialization",
        "media",
        "t",
        "d",
        "r",
        "schemeIdUri",
        "value",
        "schemaLocation",
        "lang",
        "presentationTimeOffset",
        "availabilityStartTime",
        "minimumUpdatePeriod",
        "suggestedPresentationDelay",
    ];
    let mut counts = std::collections::BTreeMap::new();
    for node in document.descendants().filter(|n| n.is_element()) {
        let name = known_tags
            .iter()
            .copied()
            .find(|name| node.has_tag_name(*name))
            .unwrap_or("unknown element (redacted)");
        *counts
            .entry(format!(
                "element {name}; namespace {}",
                namespace(node.tag_name().namespace())
            ))
            .or_insert(0usize) += 1;
        for attribute in node.attributes() {
            let name = known_attributes
                .iter()
                .copied()
                .find(|name| attribute.name() == *name)
                .unwrap_or("unknown attribute (redacted)");
            *counts.entry(format!("attribute {name}")).or_insert(0) += 1;
        }
    }
    Ok(counts
        .into_iter()
        .map(|(name, count)| format!("{name}: {count}\n"))
        .collect())
}

#[test]
fn structural_diagnostics_never_echo_unknown_names_or_values() {
    let result = structure(r#"<MPD xmlns="https://private.invalid/secret" id="private-value"><privateName privateAttribute="secret-url">private-text</privateName></MPD>"#).unwrap();
    for secret in [
        "private.invalid",
        "private-value",
        "privateName",
        "privateAttribute",
        "secret-url",
        "private-text",
    ] {
        assert!(!result.contains(secret));
    }
    assert!(result.contains("other (redacted)"));
}

pub(super) fn full(xml: &str, manifest: &Manifest) -> Result<Clip> {
    ensure!(
        !manifest.duration.is_zero() && manifest.duration <= Duration::from_secs(900),
        "Whole track exceeds the fifteen-minute test bound; no partial fallback"
    );
    ensure!(
        xml.len() <= 8192 && (8000..=96000).contains(&manifest.sample_rate),
        "Whole-track manifest or sample rate exceeds reviewed bounds"
    );
    url(&manifest.init)?;
    for segment in &manifest.segments {
        url(&segment.url)?;
    }
    let document = roxmltree::Document::parse(xml)
        .map_err(|_| anyhow::anyhow!("Invalid whole-track manifest"))?;
    ensure!(
        document.root_element().has_tag_name("MPD")
            && document
                .root_element()
                .attribute("type")
                .unwrap_or("static")
                == "static",
        "Expected one static MPD"
    );
    let allowed = [
        "MPD",
        "Period",
        "AdaptationSet",
        "Representation",
        "AudioChannelConfiguration",
        "SegmentTemplate",
        "SegmentTimeline",
        "S",
        "Role",
    ];
    for node in document.descendants().filter(|n| n.is_element()) {
        ensure!(
            node.tag_name().namespace() == Some("urn:mpeg:dash:schema:mpd:2011")
                && allowed.contains(&node.tag_name().name()),
            "Unreviewed whole-track manifest feature; no fallback"
        );
        ensure!(
            !node
                .attributes()
                .any(|a| matches!(a.name(), "href" | "base")),
            "External manifest reference rejected"
        );
        if node.has_tag_name("Role") {
            ensure!(
                node.attribute("schemeIdUri") == Some("urn:mpeg:dash:role:2011")
                    && node.attribute("value") == Some("main")
                    && node
                        .attributes()
                        .all(|a| ["schemeIdUri", "value", "id"].contains(&a.name()))
                    && node.parent().is_some_and(
                        |p| p.has_tag_name("AdaptationSet") || p.has_tag_name("Representation")
                    )
                    && node
                        .children()
                        .all(|n| n.is_text() && n.text().unwrap_or_default().trim().is_empty()),
                "Only the standard empty main-audio role is reviewed"
            );
        }
        if node.has_tag_name("SegmentTemplate") {
            ensure!(
                node.attributes().all(|a| [
                    "initialization",
                    "media",
                    "timescale",
                    "startNumber",
                    "presentationTimeOffset"
                ]
                .contains(&a.name()))
                    && node.attribute("presentationTimeOffset").unwrap_or("0") == "0",
                "Unreviewed segment-template feature rejected"
            );
        }
    }
    for tag in ["Period", "Representation", "SegmentTemplate"] {
        ensure!(
            document
                .descendants()
                .filter(|n| n.has_tag_name(tag))
                .count()
                == 1,
            "Whole-track proof requires one period, representation and template"
        );
    }
    Ok(Clip {
        xml: xml.to_owned(),
        duration: manifest.duration,
    })
}

#[test]
fn whole_track_rejects_remote_manifest_features_and_excess_duration() {
    let xml = r#"<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT3S"><Period><AdaptationSet><Representation codecs="flac" audioSamplingRate="44100"><SegmentTemplate timescale="44100" initialization="https://audio.tidal.com/fixture/0.mp4" media="https://audio.tidal.com/fixture/$Number$.mp4" startNumber="1"><SegmentTimeline><S d="44100" r="2"/></SegmentTimeline></SegmentTemplate></Representation></AdaptationSet></Period></MPD>"#;
    let mut manifest = crate::dash::parse(xml).unwrap();
    let full = full(xml, &manifest).unwrap();
    assert_eq!(full.duration, Duration::from_secs(3));
    assert_eq!(full.xml, xml);
    let with_role = xml.replace(
        "<AdaptationSet>",
        "<AdaptationSet><Role schemeIdUri=\"urn:mpeg:dash:role:2011\" value=\"main\"/>",
    );
    assert!(self::full(&with_role, &manifest).is_ok());
    assert!(
        self::full(
            &with_role.replace("value=\"main\"", "value=\"alternate\""),
            &manifest
        )
        .is_err()
    );
    assert!(
        self::full(
            &with_role.replace("urn:mpeg:dash:role:2011", "https://unreviewed.invalid/role"),
            &manifest
        )
        .is_err()
    );
    assert!(
        self::full(
            &xml.replace(
                "</MPD>",
                "<Role schemeIdUri=\"urn:mpeg:dash:role:2011\" value=\"main\"/></MPD>"
            ),
            &manifest
        )
        .is_err()
    );
    for extra in [
        "<Location>https://example.com/a</Location>",
        "<ContentProtection/>",
        "<BaseURL>https://example.com/</BaseURL>",
    ] {
        assert!(self::full(&xml.replace("</MPD>", &format!("{extra}</MPD>")), &manifest).is_err());
    }
    manifest.duration = Duration::from_secs(901);
    assert!(self::full(xml, &manifest).is_err());
}

#[test]
fn finite_clip_contains_only_complete_prefix_segments() {
    let manifest = Manifest {
        xml: std::sync::Arc::from(""),
        codec: Codec::Flac,
        init: "https://audio.tidal.com/fixture/init.mp4?sig=synthetic&x=1".into(),
        segments: (0..20)
            .map(|i| crate::dash::Segment {
                url: format!("https://audio.tidal.com/fixture/{i}.mp4"),
                start: Duration::from_secs(i * 3),
                duration: Duration::from_secs(3),
            })
            .collect(),
        sample_rate: 44100,
        duration: Duration::from_secs(60),
    };
    let clip = build(&manifest).unwrap();
    assert_eq!(clip.duration, Duration::from_secs(6));
    let document = roxmltree::Document::parse(&clip.xml).unwrap();
    assert_eq!(
        document
            .descendants()
            .filter(|n| n.has_tag_name("SegmentURL"))
            .count(),
        2
    );
    assert!(
        !document
            .descendants()
            .any(|n| n.has_tag_name("SegmentTemplate") || n.has_tag_name("BaseURL"))
    );
    assert_eq!(
        document
            .descendants()
            .find(|n| n.has_tag_name("Initialization"))
            .unwrap()
            .attribute("sourceURL"),
        Some(manifest.init.as_str())
    );
    let mut invalid = manifest;
    invalid.segments[0].duration = Duration::from_secs(11);
    assert!(build(&invalid).is_err());
}
#[test]
fn synthetic_aac_clip_for_private_decoder_check() {
    let mut frames = 0u64;
    let segments = [45056u64, 45056, 45056, 45056, 39936]
        .into_iter()
        .enumerate()
        .map(|(i, count)| {
            let segment = crate::dash::Segment {
                url: format!("https://audio.tidal.com/synthetic/{}.mp4", i + 1),
                start: Duration::from_secs_f64(frames as f64 / 44100.0),
                duration: Duration::from_secs_f64(count as f64 / 44100.0),
            };
            frames += count;
            segment
        })
        .collect();
    let manifest = Manifest {
        xml: std::sync::Arc::from(""),
        codec: Codec::AacLc,
        init: "https://audio.tidal.com/synthetic/0.mp4".into(),
        segments,
        sample_rate: 44100,
        duration: Duration::from_secs(5),
    };
    let clip = build(&manifest).unwrap();
    assert!(clip.duration > Duration::from_secs(4) && clip.duration < Duration::from_secs(6));
    if let Some(path) = std::env::var_os("TIDAL_CAST_SYNTHETIC_CLIP_OUTPUT") {
        std::fs::write(path, clip.xml).unwrap();
    }
}

#[test]
fn unreviewed_origins_and_credentials_are_rejected() {
    for value in [
        "http://audio.tidal.com/a",
        "https://audio.tidal.com.evil.invalid/a",
        "https://example.com/a",
        "https://127.0.0.1/a",
        "https://user:secret@audio.tidal.com/a",
        "https://audio.tidal.com:444/a",
        "https://audio.tidal.com/a#fragment",
    ] {
        assert!(url(value).is_err());
    }
    assert!(url("https://sp-pr-fa.audio.tidal.com/a?token=synthetic").is_ok());
}
