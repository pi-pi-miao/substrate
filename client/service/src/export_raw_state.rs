// Copyright 2020 Parity Technologies (UK) Ltd.
// This file is part of Substrate.

// Substrate is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Substrate is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Substrate.  If not, see <http://www.gnu.org/licenses/>.

use crate::error;
use crate::error::Error;
use sc_chain_spec::ChainSpec;
use log::{warn, info};
use futures::{future, prelude::*};
use sp_runtime::traits::{
	Block as BlockT, NumberFor, One, Zero, Header, SaturatedConversion, MaybeSerializeDeserialize,
};
use sp_runtime::generic::{BlockId, SignedBlock};
use codec::{Decode, Encode, IoReader as CodecIoReader};
use crate::client::{Client, LocalCallExecutor};
use sp_consensus::{
	BlockOrigin,
	import_queue::{IncomingBlock, Link, BlockImportError, BlockImportResult, ImportQueue},
};
use sc_executor::{NativeExecutor, NativeExecutionDispatch};
use sp_core::storage::{StorageKey, well_known_keys, ChildInfo, Storage, StorageChild, StorageMap};
use sc_client_api::{StorageProvider, BlockBackend, UsageProvider};

use std::{io::{Read, Write, Seek}, pin::Pin, collections::HashMap, sync::Arc};
use std::time::{Duration, Instant};
use futures_timer::Delay;
use std::task::Poll;
use serde_json::{de::IoRead as JsonIoRead, Deserializer, StreamDeserializer};
use std::convert::{TryFrom, TryInto};
use sp_runtime::traits::{CheckedDiv, Saturating};

pub fn export_raw_state<B, BA, CE, RA>(
	client: Arc<Client<BA, CE, B, RA>>,
    block: Option<BlockId<B>>,
) -> Result<Storage, Error>
where
    B: BlockT,
	BA: sc_client_api::backend::Backend<B> + 'static,
	CE: sc_client_api::call_executor::CallExecutor<B> + Send + Sync + 'static,
	RA: Send + Sync + 'static,
{
    let block = block.unwrap_or_else(
        || BlockId::Hash(client.usage_info().chain.best_hash)
    );

    let empty_key = StorageKey(Vec::new());
    let mut top_storage = client.storage_pairs(&block, &empty_key)?;
    let mut children_default = HashMap::new();

    // Remove all default child storage roots from the top storage and collect the child storage
    // pairs.
    while let Some(pos) = top_storage
        .iter()
        .position(|(k, _)| k.0.starts_with(well_known_keys::DEFAULT_CHILD_STORAGE_KEY_PREFIX)) {
        let (key, _) = top_storage.swap_remove(pos);

        let key = StorageKey(
            key.0[well_known_keys::DEFAULT_CHILD_STORAGE_KEY_PREFIX.len()..].to_vec(),
        );
        let child_info = ChildInfo::new_default(&key.0);

        let keys = client.child_storage_keys(&block, &child_info, &empty_key)?;
        let mut pairs = StorageMap::new();
        keys.into_iter().try_for_each(|k| {
            if let Some(value) = client.child_storage(&block, &child_info, &k)? {
                pairs.insert(k.0, value.0);
            }

            Ok::<_, Error>(())
        })?;

        children_default.insert(key.0, StorageChild { child_info, data: pairs });
    }

    let top = top_storage.into_iter().map(|(k, v)| (k.0, v.0)).collect();
    Ok(Storage { top, children_default })
}
