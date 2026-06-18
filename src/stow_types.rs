use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionType {
    CreateSymlink,
    DeleteSymlink,
    CreateDirectory,
    DeleteDirectory,
    AdoptFile,
    AdoptDirectory,
    Skip,
    Conflict,
}

#[derive(Debug, Clone)]
pub struct TargetAction {
    pub source_item: Option<StowItem>,
    pub target_path: PathBuf,
    pub link_target_path: Option<PathBuf>,
    pub action_type: ActionType,
    pub conflict_details: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StowItemType {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StowItem {
    pub package_relative_path: PathBuf,
    pub source_path: PathBuf,
    pub item_type: StowItemType,
    pub target_name_after_dotfiles_processing: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetActionReportStatus {
    Success,
    Skipped,
    ConflictPrevented,
    Failure(String),
}

impl TargetActionReportStatus {
    pub(crate) fn is_blocking(&self) -> bool {
        matches!(
            self,
            TargetActionReportStatus::ConflictPrevented | TargetActionReportStatus::Failure(_)
        )
    }
}

#[derive(Debug, Clone)]
pub struct TargetActionReport {
    pub original_action: TargetAction,
    pub status: TargetActionReportStatus,
    pub message: Option<String>,
}
