use std::sync::Arc;

use common_base::log::error_meta;
use serde::{Deserialize, Serialize};

use super::{keys::key_segment, rocksdb::RocksDBEngine};

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct SegmentInfo {
    pub cluster_name: String,
    pub shard_name: String,
    pub segment_seq: u64,
    pub replicas: Vec<Replica>,
    pub replica_leader: u32,
    pub status: SegmentStatus,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Replica {
    pub replica_seq: u64,
    pub node_id: u64,
    pub fold: String,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub enum SegmentStatus {
    #[default]
    Idle,
    Write,
    PrepareSealUp,
    SealUp,
}

pub struct SegmentStorage {
    rocksdb_engine_handler: Arc<RocksDBEngine>,
}

impl SegmentStorage {
    pub fn new(rocksdb_engine_handler: Arc<RocksDBEngine>) -> Self {
        SegmentStorage {
            rocksdb_engine_handler,
        }
    }

    pub fn save_segment(&self, segment: SegmentInfo) {
        let cf = self.rocksdb_engine_handler.cf_cluster();
        let shard_key = key_segment(
            &segment.cluster_name.clone(),
            &segment.shard_name.clone(),
            segment.segment_seq,
        );
        match self.rocksdb_engine_handler.write(cf, &shard_key, &segment) {
            Ok(_) => {}
            Err(e) => {
                error_meta(&e);
            }
        }
    }

    pub fn get_segment(
        &self,
        cluster_name: String,
        shard_name: String,
        segment_seq: u64,
    ) -> Option<SegmentInfo> {
        let cf = self.rocksdb_engine_handler.cf_cluster();
        let shard_key: String = key_segment(&cluster_name, &shard_name, segment_seq);
        match self
            .rocksdb_engine_handler
            .read::<SegmentInfo>(cf, &shard_key)
        {
            Ok(ci) => {
                return ci;
            }
            Err(_) => {}
        }
        return None;
    }

    pub fn delete_segment(&self, cluster_name: String, shard_name: String, segment_seq: u64) {
        let cf = self.rocksdb_engine_handler.cf_cluster();
        let shard_key = key_segment(&cluster_name, &shard_name, segment_seq);
        match self.rocksdb_engine_handler.delete(cf, &shard_key) {
            Ok(_) => {}
            Err(e) => {
                error_meta(&e);
            }
        }
    }
}
