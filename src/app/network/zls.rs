use crate::app::constants::ZLS_SELECT_VERSION_ENDPOINT;
use crate::app::network::create_client;
use crate::{NetErr, ZvError};
use color_eyre::eyre::eyre;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ZlsArtifact {
    pub tarball: String,
    pub shasum: String,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct ZlsRelease {
    pub version: String,
    pub date: String,
    pub per_target: HashMap<String, ZlsArtifact>,
}

impl ZlsRelease {
    pub fn artifact_for_target(&self, target: &str) -> Option<&ZlsArtifact> {
        self.per_target.get(target)
    }
}

#[derive(Debug, Deserialize)]
struct SelectVersionResponse {
    version: String,
    date: String,
    #[serde(flatten)]
    targets: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ArtifactDto {
    tarball: String,
    shasum: String,
    #[serde(deserialize_with = "deserialize_size")]
    size: u64,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SizeValue {
    Number(u64),
    String(String),
}

fn deserialize_size<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = SizeValue::deserialize(deserializer)?;
    match value {
        SizeValue::Number(value) => Ok(value),
        SizeValue::String(value) => value.parse::<u64>().map_err(serde::de::Error::custom),
    }
}

pub async fn select_version(zig_version: &str) -> Result<ZlsRelease, ZvError> {
    let client = create_client()?;
    let response = client
        .get(ZLS_SELECT_VERSION_ENDPOINT)
        .query(&[("zig_version", zig_version), ("compatibility", "full")])
        .send()
        .await
        .map_err(NetErr::Reqwest)
        .map_err(ZvError::NetworkError)?;

    if !response.status().is_success() {
        return Err(ZvError::NetworkError(NetErr::HTTP(response.status())));
    }

    let body: SelectVersionResponse = response
        .json()
        .await
        .map_err(NetErr::Reqwest)
        .map_err(ZvError::NetworkError)?;

    let mut per_target = HashMap::new();
    for (key, value) in body.targets {
        match serde_json::from_value::<ArtifactDto>(value) {
            Ok(artifact) => {
                per_target.insert(
                    key,
                    ZlsArtifact {
                        tarball: artifact.tarball,
                        shasum: artifact.shasum,
                        size: artifact.size,
                    },
                );
            }
            Err(err) => {
                tracing::warn!(
                    target: "zv::network::zls",
                    artifact_target = %key,
                    "Skipping malformed ZLS artifact in select-version response: {err}"
                );
            }
        }
    }

    if per_target.is_empty() {
        return Err(ZvError::General(eyre!(
            "ZLS select-version response did not include any target artifacts"
        )));
    }

    Ok(ZlsRelease {
        version: body.version,
        date: body.date,
        per_target,
    })
}
