//! The index format (schema 1). The index is a git repository with one JSON file per package,
//! `packages/<first two letters>/<name>.json` (`packages/@<scope>/<name>.json` for a scoped
//! name), and `index.json`, a summary for search. A package
//! file only grows: a new version appends an entry, and yanking or deprecating sets a flag.
//! `registry/packages-template/schema/` has the JSON Schema of both files.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA: u32 = 1;

/// The GitHub repository of the public index.
pub const DEFAULT_REPO: &str = "utkarshavardhana/sspur-packages";

/// Where clients read the index by default.
pub const DEFAULT_INDEX: &str = "https://raw.githubusercontent.com/utkarshavardhana/sspur-packages/main";

fn one() -> u32 {
    SCHEMA
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
}

/// One public definition of a version, for the API diff and the package pages.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ApiItem {
    pub name: String,
    pub kind: String,
    pub sig: String,
    #[serde(default)]
    pub sig_hash: String,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub effects: Vec<String>,
    #[serde(default)]
    pub doc: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub version: String,
    pub git: String,
    #[serde(default)]
    pub tag: String,
    pub commit: String,
    /// The package's export hash: the Merkle root over its public definitions (ADR 0026).
    pub hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Snapshot>,
    #[serde(default)]
    pub effects: Vec<String>,
    #[serde(default)]
    pub deps: BTreeMap<String, String>,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub yanked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    #[serde(default)]
    pub published: String,
    #[serde(default)]
    pub publisher: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<u64>,
    #[serde(default)]
    pub api: Vec<ApiItem>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Package {
    #[serde(default = "one")]
    pub schema: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub repository: String,
    #[serde(default)]
    pub owners: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub versions: Vec<Entry>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct SummaryItem {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub latest: Option<String>,
    #[serde(default)]
    pub versions: usize,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub updated: String,
    #[serde(default)]
    pub yanked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Summary {
    #[serde(default = "one")]
    pub schema: u32,
    #[serde(default)]
    pub packages: Vec<SummaryItem>,
}

/// `packages/mo/money.json` for `money`, and `packages/@acme/pkg.json` for the scoped name
/// `acme/pkg` (ADR 0039): a scope's directory starts with `@`, which no two-letter shard does.
pub fn package_path(name: &str) -> String {
    if let Some((scope, rest)) = name.split_once('/') {
        return format!("packages/@{scope}/{rest}.json");
    }
    let shard: String = name.chars().take(2).collect();
    format!("packages/{shard}/{name}.json")
}

impl Package {
    pub fn parse(text: &str) -> Result<Package, String> {
        let p: Package = serde_json::from_str(text).map_err(|e| format!("bad package file: {e}"))?;
        if p.schema > SCHEMA {
            return Err(format!("package file schema {} is newer than this sspur understands ({SCHEMA}); upgrade sspur", p.schema));
        }
        Ok(p)
    }

    pub fn entry(&self, version: &str) -> Option<&Entry> {
        self.versions.iter().find(|e| e.version == version)
    }
}

impl Summary {
    pub fn parse(text: &str) -> Result<Summary, String> {
        let s: Summary = serde_json::from_str(text).map_err(|e| format!("bad index.json: {e}"))?;
        if s.schema > SCHEMA {
            return Err(format!("index.json schema {} is newer than this sspur understands ({SCHEMA}); upgrade sspur", s.schema));
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_files_round_trip_and_tolerate_new_fields() {
        let t = r#"{"schema": 1, "name": "money", "owners": ["alice"], "future": 3,
          "versions": [{"version": "1.0.0", "git": "https://github.com/alice/money.git", "tag": "v1.0.0",
            "commit": "0123456789012345678901234567890123456789", "hash": "abc", "deps": {"text": "^0.2"}, "extra": true}]}"#;
        let p = Package::parse(t).unwrap();
        assert_eq!(p.versions[0].deps["text"], "^0.2");
        assert!(!p.versions[0].yanked);
        let back = Package::parse(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        assert!(Package::parse(r#"{"schema": 2, "name": "x"}"#).unwrap_err().contains("newer"));
        assert_eq!(package_path("money"), "packages/mo/money.json");
        assert_eq!(package_path("acme/money"), "packages/@acme/money.json");
    }
}
