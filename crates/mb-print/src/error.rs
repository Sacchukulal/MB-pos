//! What printing can refuse to do.
//!
//! There is exactly one of these, and it is small on purpose: almost everything
//! that could go wrong on a receipt has an answer that is better than an error
//! — wrap it, cap it, clamp it. The one case with no sensible answer is an
//! amount too wide to print, and that one is genuinely an error.

/// Something a document cannot be made to do.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PrintError {
    /// Rule three's last clause: the amount alone is wider than the paper.
    ///
    /// Truncating it would print a bill whose lines do not add up to its total,
    /// which is the one output requirement 7 of the ten forbids outright. On
    /// 58 mm paper this means an amount over about ten crore, so a shop that
    /// sees it has either typed something wrong or needs wider paper — and both
    /// are worth being told.
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
