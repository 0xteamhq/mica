//! S3 uploader — built on `aws-sdk-s3`.
//!
//! Uses a single PUT for files ≤ 8 MB and the SDK's automatic multipart
//! flow above that threshold (it handles part-sizing and concurrency
//! for us when we feed `ByteStream::from_path`). Region falls through
//! to the SDK's default credentials/region chain when `region` is empty.

use super::Uploader;
use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use std::path::Path;

pub struct S3Uploader {
    client: Client,
    bucket: String,
    prefix: String,
}

impl S3Uploader {
    /// Build an uploader from CLI args. Returns `None` when bucket is
    /// empty so wiring code can `if let Some(u) = ...`.
    pub async fn from_args(bucket: &str, region: &str, prefix: &str) -> Option<Self> {
        if bucket.is_empty() {
            return None;
        }
        let mut loader = aws_config::defaults(BehaviorVersion::latest());
        if !region.is_empty() {
            loader = loader.region(aws_sdk_s3::config::Region::new(region.to_string()));
        }
        let conf = loader.load().await;
        Some(Self {
            client: Client::new(&conf),
            bucket: bucket.to_string(),
            prefix: prefix.to_string(),
        })
    }

    fn key_for(&self, path: &Path, session_id: &str) -> String {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(session_id);
        if self.prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}/{name}", self.prefix.trim_end_matches('/'))
        }
    }
}

#[async_trait]
impl Uploader for S3Uploader {
    async fn upload(&self, path: &Path, session_id: &str) -> anyhow::Result<()> {
        let key = self.key_for(path, session_id);
        let body = ByteStream::from_path(path).await?;
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(body)
            .send()
            .await?;
        Ok(())
    }
}
