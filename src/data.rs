use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default,Serialize, Deserialize)]
pub struct Data{
    pub topisc: BTreeMap<String,Vec<String>>,
}

