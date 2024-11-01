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

use std::sync::Arc;

use common_base::config::journal_server::journal_server_conf;
use dashmap::DashMap;
use grpc_clients::placement::journal::call::{list_segment, list_shard};
use grpc_clients::placement::placement::call::node_list;
use grpc_clients::pool::ClientPool;
use log::{error, info};
use metadata_struct::journal::segment::JournalSegment;
use metadata_struct::journal::shard::JournalShard;
use metadata_struct::placement::node::BrokerNode;
use protocol::journal_server::journal_inner::{
    JournalUpdateCacheActionType, JournalUpdateCacheResourceType,
};
use protocol::placement_center::placement_center_inner::NodeListRequest;
use protocol::placement_center::placement_center_journal::{ListSegmentRequest, ListShardRequest};

use super::cluster::JournalEngineClusterConfig;
use super::shard::delete_shard;

#[derive(Clone)]
pub struct CacheManager {
    pub cluster: DashMap<String, JournalEngineClusterConfig>,
    pub node_list: DashMap<u64, BrokerNode>,
    shards: DashMap<String, JournalShard>,
    segments: DashMap<String, DashMap<u32, JournalSegment>>,
}

impl CacheManager {
    pub fn new() -> Self {
        let cluster = DashMap::with_capacity(2);
        let node_list = DashMap::with_capacity(2);
        let shards = DashMap::with_capacity(8);
        let segments = DashMap::with_capacity(8);
        CacheManager {
            cluster,
            node_list,
            shards,
            segments,
        }
    }

    pub fn add_node(&self, node: BrokerNode) {
        self.node_list.insert(node.node_id, node);
    }

    pub fn get_cluster(&self) -> JournalEngineClusterConfig {
        return self.cluster.get("local").unwrap().clone();
    }

    pub fn init_cluster(&self) {
        let cluster = JournalEngineClusterConfig::default();
        self.cluster.insert("local".to_string(), cluster);
    }

    pub fn add_shard(&self, shard: JournalShard) {
        let key = self.shard_key(&shard.namespace, &shard.shard_name);
        self.shards.insert(key, shard);
    }

    pub fn get_shard(&self, namespace: &str, shard_name: &str) -> Option<JournalShard> {
        let key = self.shard_key(namespace, shard_name);
        if let Some(shard) = self.shards.get(&key) {
            return Some(shard.clone());
        }
        None
    }

    pub fn delete_shard(&self, namespace: &str, shard_name: &str) {
        let key = self.shard_key(namespace, shard_name);
        self.shards.remove(&key);
        self.segments.remove(&key);
    }

    pub fn get_active_segment(&self, namespace: &str, shard_name: &str) -> Option<JournalSegment> {
        let key = self.shard_key(namespace, shard_name);
        if let Some(shard) = self.shards.get(&key) {
            if let Some(segment) = self.get_segment(namespace, shard_name, shard.active_segment_seq)
            {
                if !segment.is_seal_up() {
                    return Some(segment);
                }
            }
        }

        None
    }

    pub fn add_segment(&self, segment: JournalSegment) {
        let key = self.shard_key(&segment.namespace, &segment.shard_name);
        if let Some(segment_list) = self.segments.get(&key) {
            segment_list.insert(segment.segment_seq, segment);
        } else {
            let data = DashMap::with_capacity(2);
            data.insert(segment.segment_seq, segment);
            self.segments.insert(key, data);
        }
    }

    pub fn delete_segment(&self, segment: JournalSegment) {
        let key = self.shard_key(&segment.namespace, &segment.shard_name);
        if let Some(segment_list) = self.segments.get(&key) {
            segment_list.remove(&segment.segment_seq);
        }
    }

    pub fn get_segment(
        &self,
        namespace: &str,
        shard_name: &str,
        segment_no: u32,
    ) -> Option<JournalSegment> {
        let key = self.shard_key(namespace, shard_name);
        if let Some(sgement_list) = self.segments.get(&key) {
            if let Some(segment) = sgement_list.get(&segment_no) {
                return Some(segment.clone());
            }
        }
        None
    }

    pub fn shard_exists(&self, namespace: &str, shard_name: &str) -> bool {
        let key = self.shard_key(namespace, shard_name);
        self.shards.contains_key(&key)
    }

    fn shard_key(&self, namespace: &str, shard_name: &str) -> String {
        format!("{}_{}", namespace, shard_name)
    }

    pub fn update_cache(
        &self,
        action_type: JournalUpdateCacheActionType,
        resource_type: JournalUpdateCacheResourceType,
        data: &str,
    ) {
        match resource_type {
            JournalUpdateCacheResourceType::JournalNode => self.parse_node(action_type, data),
            JournalUpdateCacheResourceType::Shard => self.parse_shard(action_type, data),
            JournalUpdateCacheResourceType::Segment => self.parse_segment(action_type, data),
        }
    }

    fn parse_node(&self, action_type: JournalUpdateCacheActionType, data: &str) {
        match action_type {
            JournalUpdateCacheActionType::Add => match serde_json::from_str::<BrokerNode>(data) {
                Ok(node) => {
                    info!("Update the cache, add node, node id: {}", node.node_id);
                    self.node_list.insert(node.node_id, node);
                }
                Err(e) => {
                    error!(
                        "Add node information failed to parse with error message :{},body:{}",
                        e, data,
                    );
                }
            },

            JournalUpdateCacheActionType::Delete => {
                match serde_json::from_str::<BrokerNode>(data) {
                    Ok(node) => {
                        info!("Update the cache, remove node, node id: {}", node.node_id);
                        self.node_list.remove(&node.node_id);
                    }
                    Err(e) => {
                        error!(
                        "Remove node information failed to parse with error message :{},body:{}",
                        e, data,
                    );
                    }
                }
            }
        }
    }

    fn parse_shard(&self, action_type: JournalUpdateCacheActionType, data: &str) {
        match action_type {
            JournalUpdateCacheActionType::Add => match serde_json::from_str::<JournalShard>(data) {
                Ok(shard) => {
                    info!(
                        "Update the cache, add shard, shard name: {}",
                        shard.shard_name
                    );
                    self.add_shard(shard);
                }
                Err(e) => {
                    error!(
                        "Add shard information failed to parse with error message :{},body:{}",
                        e, data,
                    );
                }
            },

            JournalUpdateCacheActionType::Delete => {
                match serde_json::from_str::<JournalShard>(data) {
                    Ok(shard) => {
                        info!(
                            "Update the cache, remove shard, shard name: {}",
                            shard.shard_name
                        );

                        // Remove the shard and Segment information from the cache
                        self.delete_shard(&shard.namespace, &shard.shard_name);

                        // Delete the local segment file asynchronously
                        tokio::spawn(async move {
                            match delete_shard() {
                                Ok(()) => {}
                                Err(e) => {}
                            }
                        });
                    }
                    Err(e) => {
                        error!(
                            "Remove shard information failed to parse with error message :{},body:{}",
                            e, data,
                        );
                    }
                }
            }
        }
    }

    fn parse_segment(&self, action_type: JournalUpdateCacheActionType, data: &str) {
        match action_type {
            JournalUpdateCacheActionType::Add => {
                match serde_json::from_str::<JournalSegment>(data) {
                    Ok(segment) => {
                        info!(
                            "Update the cache, add segment, shard name: {}, segment no:{}",
                            segment.shard_name, segment.segment_seq
                        );
                        self.add_segment(segment);
                    }
                    Err(e) => {
                        error!(
                            "Add segment information failed to parse with error message :{},body:{}",
                            e, data,
                        );
                    }
                }
            }

            JournalUpdateCacheActionType::Delete => {
                match serde_json::from_str::<JournalSegment>(data) {
                    Ok(segment) => {
                        info!(
                            "Update the cache, remove segment, shard name: {}, segment no:{}",
                            segment.shard_name, segment.segment_seq
                        );
                        self.delete_segment(segment);
                    }
                    Err(e) => {
                        error!(
                            "Remove segment information failed to parse with error message :{},body:{}",
                            e, data,
                        );
                    }
                }
            }
        }
    }
}

pub async fn load_cache(client_pool: &Arc<ClientPool>, cache_manager: &Arc<CacheManager>) {
    let conf = journal_server_conf();
    // load node
    let request = NodeListRequest {
        cluster_name: conf.cluster_name.clone(),
    };
    match node_list(client_pool.clone(), conf.placement_center.clone(), request).await {
        Ok(list) => {
            info!(
                "Load the node cache, the number of nodes is {}",
                list.nodes.len()
            );
            for raw in list.nodes {
                let node = match serde_json::from_slice::<BrokerNode>(&raw) {
                    Ok(data) => data,
                    Err(e) => {
                        panic!("Failed to decode the BrokerNode information, {}", e);
                    }
                };
                cache_manager.add_node(node);
            }
        }
        Err(e) => {
            panic!(
                "Loading the node cache from the Placement Center failed, {}",
                e
            );
        }
    }

    // load shard
    let request = ListShardRequest {
        cluster_name: conf.cluster_name.clone(),
        ..Default::default()
    };
    match list_shard(client_pool.clone(), conf.placement_center.clone(), request).await {
        Ok(list) => match serde_json::from_slice::<Vec<JournalShard>>(&list.shards) {
            Ok(data) => {
                info!(
                    "Load the shard cache, the number of shards is {}",
                    data.len()
                );
                for shard in data {
                    cache_manager.add_shard(shard);
                }
            }
            Err(e) => {
                panic!("Failed to decode the JournalShard information, {}", e);
            }
        },
        Err(e) => {
            panic!(
                "Loading the shardcache from the Placement Center failed, {}",
                e
            );
        }
    }

    // load segment
    let request = ListSegmentRequest {
        cluster_name: conf.cluster_name.clone(),
        ..Default::default()
    };
    match list_segment(client_pool.clone(), conf.placement_center.clone(), request).await {
        Ok(list) => match serde_json::from_slice::<Vec<JournalSegment>>(&list.segments) {
            Ok(data) => {
                info!(
                    "Load the segment cache, the number of segments is {}",
                    data.len()
                );
                for shard in data {
                    cache_manager.add_segment(shard);
                }
            }
            Err(e) => {
                panic!("Failed to decode the JournalShard information, {}", e);
            }
        },
        Err(e) => {
            panic!(
                "Loading the shardcache from the Placement Center failed, {}",
                e
            );
        }
    }
    // load group
}
