# fabrials-store-sqlite

SQLite and file-backed implementations of the storage ports defined in
`fabrials_fabric::ports` (`HopStore`, `HistoryStore`, `UsageStore`,
`CredentialJournal`, `DeliveryStore`), plus the append-only JSONL hop ledger,
recoverable account-file transactions (`FileSet`) and the approved API-key
import that writes through them.
