# Design

`ConversationItem` already stores assistant and visible-reasoning text as `Arc<str>`. The projection record will hold tagged `Arc<str>` chunks cloned from those items, retaining the same underlying allocation during the live admission gate. It will also carry the source session ID needed to expand an independent resident-delta record. Version 2 of the record serializes the compact chunks; prior projection schema is not accepted as an exact current record.

At replay boundaries, expand each chunk into the ACP notification expected by the client, adding the existing projection metadata and stripping inherited identity when a fork turns the projection into child history. Only those presentation boundaries allocate owned ACP text. The digest remains over canonical Timeline items, and exact-match comparison still checks the projection payload in addition to identity and version.
