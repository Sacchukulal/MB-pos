-- Raw tendered cash belongs on the receipt; cash already returned as change does
-- not belong in the payment ledger. The migration engine invokes the same core
-- receipt projector used for new bills inside this migration's transaction.
-- Original settlement snapshots are retained in billing_account.issued_receipt.
-- Bill totals, taxes, numbers, refund records and staff/order history are unchanged.
SELECT 1;
