// Copyright 2023 RobustMQ Team
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![allow(dead_code, unused_variables)]

use std::sync::Arc;
use std::time::Duration;

use cache::{load_node_cache, load_shards_cache, start_update_cache_thread, MetadataCache};
use connection::{start_conn_gc_thread, ConnectionManager};
use error::JournalClientError;
use log::error;
use metadata_struct::journal::shard::shard_name_iden;
use option::JournalClientOption;
use protocol::journal_server::journal_engine::{CreateShardReqBody, DeleteShardReqBody};
use service::{create_shard, delete_shard};
use tokio::sync::broadcast::{self, Sender};
use tokio::time::sleep;
use writer::{SenderMessage, SenderMessageResp, Writer};

mod cache;
mod connection;
mod error;
mod group;
pub mod option;
mod reader;
mod service;
pub mod tool;
mod writer;

#[derive(Clone)]
pub struct JournalEngineClient {
    connection_manager: Arc<ConnectionManager>,
    metadata_cache: Arc<MetadataCache>,
    writer: Arc<Writer>,
    stop_send: Sender<bool>,
}

impl JournalEngineClient {
    pub fn new(options: JournalClientOption) -> Self {
        let metadata_cache = Arc::new(MetadataCache::new(options.addrs));
        let connection_manager = Arc::new(ConnectionManager::new(metadata_cache.clone()));
        let sender = Arc::new(Writer::new(
            connection_manager.clone(),
            metadata_cache.clone(),
        ));
        let (stop_send, _) = broadcast::channel::<bool>(2);
        JournalEngineClient {
            metadata_cache,
            connection_manager,
            writer: sender,
            stop_send,
        }
    }

    pub async fn connect(&self) -> Result<(), JournalClientError> {
        load_node_cache(&self.metadata_cache, &self.connection_manager).await?;

        start_update_cache_thread(
            self.metadata_cache.clone(),
            self.connection_manager.clone(),
            self.stop_send.subscribe(),
        );

        start_conn_gc_thread(
            self.metadata_cache.clone(),
            self.connection_manager.clone(),
            self.stop_send.subscribe(),
        );
        Ok(())
    }

    pub async fn create_shard(
        &self,
        namespace: &str,
        shard_name: &str,
        replica_num: u32,
    ) -> Result<(), JournalClientError> {
        let body = CreateShardReqBody {
            namespace: namespace.to_string(),
            shard_name: shard_name.to_string(),
            replica_num,
        };
        let _ = create_shard(&self.connection_manager, body).await?;
        Ok(())
    }

    pub async fn delete_shard(
        &self,
        namespace: &str,
        shard_name: &str,
    ) -> Result<(), JournalClientError> {
        let body = DeleteShardReqBody {
            namespace: namespace.to_string(),
            shard_name: shard_name.to_string(),
        };
        let _ = delete_shard(&self.connection_manager, body).await?;
        Ok(())
    }

    pub async fn send(
        &self,
        namespace: String,
        shard_name: String,
        key: String,
        content: Vec<u8>,
        tags: Vec<String>,
    ) -> Result<SenderMessageResp, JournalClientError> {
        loop {
            let active_segment = if let Some(segment) = self
                .metadata_cache
                .get_active_segment(&namespace, &shard_name)
            {
                segment
            } else {
                if let Err(e) = load_shards_cache(
                    &self.metadata_cache,
                    &self.connection_manager,
                    &namespace,
                    &shard_name,
                )
                .await
                {
                    error!(
                        "Loading Shard {} Metadata info failed, error message :{}",
                        shard_name_iden(&namespace, &shard_name,),
                        e
                    );
                }
                error!(
                    "{}",
                    JournalClientError::NotActiveSegmentLeader(shard_name_iden(
                        &namespace,
                        &shard_name,
                    ))
                );
                sleep(Duration::from_millis(100)).await;
                continue;
            };

            let message = SenderMessage::build(
                &namespace,
                &shard_name,
                active_segment,
                &key,
                &content,
                &tags,
            );
            match self.writer.send(&message).await {
                Ok(resp) => {
                    return Ok(resp);
                }
                Err(e) => {
                    if let JournalClientError::NotActiveSegmentLeader(_) = e {
                        error!("Message failed to send. Reason :{} Next attempt.", e);
                        sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                    return Err(e);
                }
            }
        }
    }

    pub async fn read_by_offset(&self, group_name: &str, namespace: &str, shard_name: &str) {}

    pub async fn read_by_timestamp(&self) {}

    pub async fn read_by_key(&self) {}

    pub async fn read_by_tag(&self) {}

    pub async fn close(&self) {
        if let Err(e) = self.stop_send.send(true) {
            error!("{}", e);
        }
        self.connection_manager.close().await;
        if let Err(e) = self.writer.close().await {
            error!("{}", e);
        }
    }
}
