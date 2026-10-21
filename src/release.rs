//! `Tag`, a release's tag, and `Build`, what this binary is: a release's or a source build

#![cfg_attr(
    not(test),
    expect(dead_code, reason = "saaio update uses it from a later task on")
)]

pub mod source;

use std::cmp::Ordering;
use std::fmt;

use anyhow::{Result, anyhow};
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A release's tag: v0.1.1, or v0.1.2-rc.1 for a pre-release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    version: Version,
}

impl Tag {
    /// `v0.1.1` or `0.1.1`; an error names anything else.
    pub fn parse(text: &str) -> Result<Tag> {
        let text = text.trim();
        let version = text.strip_prefix('v').unwrap_or(text);
        Version::parse(version)
            .map(|version| Tag { version })
            .map_err(|_| anyhow!("not a release tag: {text}"))
    }

    pub fn version(&self) -> &Version {
        &self.version
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.version)
    }
}

/// Semantic versioning's order: 0.1.1-rc.1 < 0.1.1 < 0.1.2.
impl Ord for Tag {
    fn cmp(&self, other: &Tag) -> Ordering {
        self.version.cmp(&other.version)
    }
}

impl PartialOrd for Tag {
    fn partial_cmp(&self, other: &Tag) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Serialize for Tag {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Tag {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Tag, D::Error> {
        let text = String::deserialize(deserializer)?;
        Tag::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// What this binary is: built by the release workflow from a tag, or from source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Build {
    Release(Tag),
    Source(Version),
}

impl Build {
    /// This binary: SAAIO_RELEASE, which release.yml sets, else the package's version.
    pub fn running() -> Build {
        Build::of(env!("SAAIO_RELEASE"), env!("CARGO_PKG_VERSION"))
    }

    /// `stamp` is the release tag the build was given, empty for a source build; `package` the
    /// package's version.
    fn of(stamp: &str, package: &str) -> Build {
        match Tag::parse(stamp) {
            Ok(tag) => Build::Release(tag),
            _ => Build::Source(Version::parse(package).unwrap_or_else(|_| Version::new(0, 0, 0))),
        }
    }

    pub fn version(&self) -> &Version {
        match self {
            Build::Release(tag) => tag.version(),
            Build::Source(version) => version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(text: &str) -> Tag {
        Tag::parse(text).unwrap()
    }

    #[test]
    fn reads_a_tag_with_or_without_its_v() {
        assert_eq!(tag("v0.1.1").to_string(), "v0.1.1");
        assert_eq!(tag("0.1.1\n").to_string(), "v0.1.1");
        assert_eq!(tag("v0.1.2-rc.1").to_string(), "v0.1.2-rc.1");
        assert_eq!(
            Tag::parse("latest").unwrap_err().to_string(),
            "not a release tag: latest"
        );
        assert!(Tag::parse("v0.1").is_err());
    }

    #[test]
    fn orders_tags_as_semantic_versioning_does() {
        assert!(tag("v0.1.1-rc.1") < tag("v0.1.1"));
        assert!(tag("v0.1.1") < tag("v0.1.2-rc.1"));
        assert!(tag("v0.1.9") < tag("v0.1.10"));
        assert!(tag("v0.2.0") > tag("v0.1.10"));
    }

    #[test]
    fn writes_a_tag_as_its_text() {
        let json = serde_json::to_string(&tag("v0.1.1")).unwrap();
        assert_eq!(json, "\"v0.1.1\"");
        assert_eq!(serde_json::from_str::<Tag>(&json).unwrap(), tag("v0.1.1"));
        assert!(serde_json::from_str::<Tag>("\"nonsense\"").is_err());
    }

    #[test]
    fn a_build_with_a_release_stamp_is_that_release() {
        assert_eq!(
            Build::of("v0.1.1-rc.1", "0.1.1"),
            Build::Release(tag("v0.1.1-rc.1"))
        );
        assert_eq!(
            Build::of("v0.1.1-rc.1", "0.1.1").version(),
            &Version::parse("0.1.1-rc.1").unwrap()
        );
        assert_eq!(Build::of("", "0.1.0"), Build::Source(Version::new(0, 1, 0)));
        assert_eq!(
            Build::of("garbage", "0.1.0"),
            Build::Source(Version::new(0, 1, 0))
        );
    }
}
