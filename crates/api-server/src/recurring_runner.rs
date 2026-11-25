//! D61: makes the DRAFT invoices of recurring schedules — a few seconds after start-up, then
//! every hour while the app runs — and writes what it made (or could not make) to the
//! activity log.

use app_core::time::today_local;
use database::Db;
use std::time::Duration;

pub fn spawn(db: Db) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        loop {
            run_and_record(&db).await;
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    })
}

/// One run: every schedule that has come due. Failures are logged, never raised.
pub async fn run_and_record(db: &Db) {
    let reports = match operations::recurring::run_due(db, &today_local().to_string()).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("recurring invoices could not run: {}", e.user_message());
            return;
        }
    };
    for r in reports {
        let (action, detail) = match (&r.error, r.invoice_numbers.len()) {
            (Some(e), _) => (format!("Could not make the recurring invoice \"{}\"", r.name), Some(e.clone())),
            (None, 0) => continue,
            (None, 1) => (format!("Made a recurring invoice for \"{}\"", r.name), Some(r.invoice_numbers.join(", "))),
            (None, n) => {
                (format!("Made {n} recurring invoices for \"{}\"", r.name), Some(r.invoice_numbers.join(", ")))
            }
        };
        tracing::info!(schedule = %r.recurring_id, "{action}");
        audit::record(
            db,
            audit::NewEntry {
                user_id: None,
                username: Some("system".into()),
                action,
                method: None,
                path: None,
                status_code: None,
                detail,
            },
        )
        .await;
    }
}
