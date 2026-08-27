//! What printing can refuse to do.

/// Something a document cannot be made to do.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PrintError {
    /// Rule three's last clause: the amount alone is wider than the paper.
    #[error(
        "the amount {amount} needs more than the {columns} characters this \
         paper has — use wider paper, or check the amount"
    )]
    AmountTooWide { amount: String, columns: usize },

    /// A template asked for something the document model cannot express.
    #[error("{0}")]
    Invalid(String),
}

impl PrintError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        PrintError::Invalid(message.into())
    }
}
