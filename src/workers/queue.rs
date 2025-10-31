use serde::{Deserialize, Serialize};
use strum::{AsRefStr, EnumString};

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, AsRefStr, EnumString)]
#[serde(rename_all = "lowercase")]
pub enum QueueStatus {
    #[default]
    Pending,
    Processing,
    Completed,
    Failed,
}
