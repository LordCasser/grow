//! Complete original presentation rows for a child seed. Interactive pages
//! split text and collapse Tool updates, while the new Seed preserves both.
use super::*;

impl IndexedHistory {
    pub(crate) fn append_seed_history(
        &self,
        session: &acp::SessionId,
        history: &HistorySnapshot,
        updates: &mut Vec<((u64, u64), SessionUpdate)>,
    ) -> io::Result<()> {
        let mut reader = self.page_reader()?;
        let mut start = 0;
        let records = self.records();
        while start < records {
            let visible = self.visible_range_after(start, records - 1)?;
            if visible.is_empty() {
                break;
            }
            let last = *visible.end();
            let mut after = (*visible.start(), 0);
            'responses: loop {
                let rows = reader.response_rows_after(
                    after,
                    history::HISTORY_PAGE_ROWS,
                    history.response_branch,
                )?;
                if rows.is_empty() {
                    break;
                }
                for row in rows {
                    let (record, part) = row.position();
                    if record > last {
                        break 'responses;
                    }
                    after = (record, part + 1);
                    let (_, mut update) =
                        reader.read_response_row(row, None, session, history, u64::MAX)?;
                    if let SessionUpdate::Acp(notification) = &mut update
                        && let Some(meta) = &mut notification.meta
                    {
                        for key in [
                            "grow/historyResponseEncodedStartByte",
                            "grow/historyResponseEncodedEndByte",
                            "grow/historyResponseTotalBytes",
                        ] {
                            meta.remove(key);
                        }
                    }
                    updates.push(((record, part), update));
                }
            }
            let mut after = (*visible.start(), 0);
            'echoes: loop {
                let rows = reader.user_echo_rows_after(after, history::HISTORY_PAGE_ROWS)?;
                if rows.is_empty() {
                    break;
                }
                for row in rows {
                    let (record, _) = row.position();
                    if record > last {
                        break 'echoes;
                    }
                    after = (record + 1, 0);
                    updates.push((
                        (record, 0),
                        reader.read_user_echo_row(row, session, history)?,
                    ));
                }
            }
            for reference in self.tools.seed_display(visible)? {
                if let Some(update) = reader.project_display(reference, session, history, false)? {
                    updates.push(((reference.position, 0), update));
                }
            }
            start = last + 1;
        }
        Ok(())
    }
}
