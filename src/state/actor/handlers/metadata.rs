use super::*;
use crate::state::actor::types::{MetadataCommand, MetadataResult};
use std::collections::HashMap;

impl ChannelActor {
    pub fn handle_metadata(&mut self, command: MetadataCommand) -> MetadataResult {
        match command {
            MetadataCommand::Get { key } => {
                let mut map = HashMap::new();
                if let Some(val) = self.metadata.get(&key) {
                    map.insert(key, val.clone());
                }
                Ok(map)
            }
            MetadataCommand::Set { key, value } => {
                if let Some(val) = value {
                    if self.metadata.len() >= 100 && !self.metadata.contains_key(&key) {
                        return Err(ChannelError::Generic("Metadata limit exceeded".to_string()));
                    }
                    if key.len() > 100 || val.len() > 400 {
                        return Err(ChannelError::Generic(
                            "Metadata key/value too long".to_string(),
                        ));
                    }
                    self.metadata.insert(key, val);
                } else {
                    self.metadata.remove(&key);
                }
                Ok(HashMap::new())
            }
            MetadataCommand::List => Ok(self.metadata.clone()),
        }
    }
}
