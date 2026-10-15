use crate::docs::{self, DocFields};
use crate::records::{self, EmployeeFields};
use crate::salaries::{self, BankFields, SalaryFields};
use crate::{extension, Actor, Upload};
use app_core::error::ErrorMessage;
use app_core::money::Money;
use app_core::{AppError, AppResult};
use database::Db;
use documents::{FileStore, StorageKey};

struct Env {
    _dir: tempfile::TempDir,
    db: Db,
    store: FileStore,
}

async fn env() -> AppResult<Env> {
    let dir = tempfile::tempdir().map_err(AppError::internal)?;
    let db = Db::open(&dir.path().join("t.db")).await?;
    let key = StorageKey::from_bytes(&[9u8; 32]).map_err(AppError::internal)?;
    let store = FileStore::new(dir.path().join("files"), key);
    Ok(Env { _dir: dir, db, store })
}

fn fields(staff_no: &str, name: &str) -> EmployeeFields {
    EmployeeFields { employee_id: Some(staff_no.into()), name: Some(name.into()), ..Default::default() }
}

fn pdf(name: &str, body: &[u8]) -> Option<Upload> {
    Some(Upload { original_name: name.into(), mime_type: "application/pdf".into(), bytes: body.to_vec() })
}

fn admin() -> Actor {
    Actor { id: None, username: Some("admin".into()) }
}

fn money(rupees: i64) -> Option<Money> {
    Some(Money::from_cents(rupees * 100))
}

#[test]
fn extensions_follow_the_reference_rule() {
    assert_eq!(extension("Face.PNG"), ".png");
    assert_eq!(extension("Degree v2.pdf"), ".pdf");
    assert_eq!(extension("archive.tar.gz"), ".gz");
    assert_eq!(extension(".bashrc"), "");
    assert_eq!(extension("noext"), "");
    assert_eq!(extension("weird.toolongext"), "");
    assert_eq!(extension("bad.p-f"), "");
    assert_eq!(extension("dir.d/file"), "");
}

#[tokio::test]
async fn staff_numbers_are_unique_ignoring_case() -> AppResult<()> {
    let e = env().await?;
    let a = records::create(&e.db, fields("CEN-001", "Asha")).await?;
    assert_eq!(a.status, "ACTIVE");
    assert_eq!(a.employment_type, "PERMANENT");
    assert_eq!(a.created_at, a.updated_at);
    let err = records::create(&e.db, fields("cen-001", "Other")).await;
    assert!(
        matches!(err, Err(AppError::BadRequest(ref m)) if *m == ErrorMessage::Text("Employee ID \"cen-001\" is already used by Asha".into()))
    );
    let b = records::create(&e.db, fields("CEN-002", "Nimal")).await?;
    let clash =
        records::update(&e.db, &b.id, EmployeeFields { employee_id: Some("Cen-001".into()), ..Default::default() })
            .await;
    assert!(clash.is_err());
    // Changing only the case of one's own number is allowed.
    let same =
        records::update(&e.db, &a.id, EmployeeFields { employee_id: Some("cen-001".into()), ..Default::default() })
            .await?;
    assert_eq!(same.employee_id, "cen-001");
    Ok(())
}

#[tokio::test]
async fn search_and_status_filter() -> AppResult<()> {
    let e = env().await?;
    let mut f = fields("CEN-001", "Asha");
    f.department = Some(Some("Ops".into()));
    records::create(&e.db, f).await?;
    let mut g = fields("CEN-002", "Bimal");
    g.status = Some("ON_LEAVE".into());
    records::create(&e.db, g).await?;
    assert_eq!(records::find_all(&e.db, Some("  ops "), None).await?.len(), 1);
    assert_eq!(records::find_all(&e.db, None, Some("ON_LEAVE")).await?.len(), 1);
    assert_eq!(records::find_all(&e.db, Some("cen"), None).await?.len(), 2);
    assert_eq!(records::find_all(&e.db, Some(""), None).await?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn documents_are_encrypted_versioned_and_restored_on_delete() -> AppResult<()> {
    let e = env().await?;
    let emp = records::create(&e.db, fields("CEN-001", "Asha")).await?;
    let d1 = docs::add(
        &e.db,
        &e.store,
        &emp.id,
        DocFields { title: Some("Degree".into()), category: Some("CERTIFICATE".into()), ..Default::default() },
        pdf("Degree.pdf", b"%PDF-1.4 confidential degree"),
        &admin(),
    )
    .await?;
    assert!(d1.file_key.ends_with(".pdf"));
    let raw = e.store.raw(&d1.file_key)?;
    assert_eq!(&raw[..4], b"QBE1");
    assert!(!raw.windows(12).any(|w| w == b"confidential"));

    let d2 =
        docs::replace(&e.db, &e.store, &emp.id, &d1.id, DocFields::default(), pdf("v2.pdf", b"v2"), &admin()).await?;
    assert_eq!(d2.title, "Degree");
    assert_eq!(d2.category, "CERTIFICATE");
    assert_eq!(d2.replaces_id.as_deref(), Some(d1.id.as_str()));
    let again =
        docs::replace(&e.db, &e.store, &emp.id, &d1.id, DocFields::default(), pdf("v3.pdf", b"v3"), &admin()).await;
    assert!(matches!(again, Err(AppError::BadRequest(_))), "older versions cannot be replaced");

    let d3 =
        docs::replace(&e.db, &e.store, &emp.id, &d2.id, DocFields::default(), pdf("v3.pdf", b"v3"), &admin()).await?;
    // Deleting the middle version re-points v3 at v1.
    docs::remove(&e.db, &e.store, &emp.id, &d2.id).await?;
    let list = docs::list(&e.db, &emp.id).await?;
    let v3 = list.iter().find(|d| d.id == d3.id).ok_or_else(|| AppError::internal("v3"))?;
    assert_eq!(v3.replaces_id.as_deref(), Some(d1.id.as_str()));
    // Deleting the current version makes the one it replaced current again.
    docs::remove(&e.db, &e.store, &emp.id, &d3.id).await?;
    let list = docs::list(&e.db, &emp.id).await?;
    assert_eq!(list.len(), 1);
    assert!(list[0].is_current);
    assert!(matches!(e.store.get(&d3.file_key), Err(AppError::NotFound(_))), "file removed");
    Ok(())
}

#[tokio::test]
async fn upload_checks_and_cleanup_on_employee_delete() -> AppResult<()> {
    let e = env().await?;
    let emp = records::create(&e.db, fields("CEN-001", "Asha")).await?;
    let no_file = docs::add(&e.db, &e.store, &emp.id, DocFields::default(), None, &admin()).await;
    assert!(
        matches!(no_file, Err(AppError::BadRequest(ref m)) if *m == ErrorMessage::Text("Choose a file to upload".into()))
    );
    let zip =
        Some(Upload { original_name: "a.zip".into(), mime_type: "application/zip".into(), bytes: b"PK".to_vec() });
    assert!(docs::add(&e.db, &e.store, &emp.id, DocFields::default(), zip, &admin()).await.is_err());
    let txt = Some(Upload { original_name: "a.txt".into(), mime_type: "text/plain".into(), bytes: b"x".to_vec() });
    assert!(records::set_photo(&e.db, &e.store, &emp.id, txt).await.is_err());

    let png =
        |n: &str| Some(Upload { original_name: n.into(), mime_type: "image/png".into(), bytes: b"\x89PNG".to_vec() });
    let first = records::set_photo(&e.db, &e.store, &emp.id, png("a.PNG")).await?;
    let first_key = first.photo_key.clone().unwrap_or_default();
    let second = records::set_photo(&e.db, &e.store, &emp.id, png("b.png")).await?;
    assert!(matches!(e.store.get(&first_key), Err(AppError::NotFound(_))), "old photo removed");
    let photo = records::get_photo(&e.db, &e.store, &emp.id).await?;
    assert_eq!(photo.bytes, b"\x89PNG");
    assert_eq!(photo.content_type, "image/png");

    let doc = docs::add(&e.db, &e.store, &emp.id, DocFields::default(), pdf("x.pdf", b"x"), &admin()).await?;
    records::remove(&e.db, &e.store, &emp.id).await?;
    assert!(matches!(e.store.get(&doc.file_key), Err(AppError::NotFound(_))));
    assert!(matches!(e.store.get(&second.photo_key.unwrap_or_default()), Err(AppError::NotFound(_))));
    Ok(())
}

#[tokio::test]
async fn salary_history_states_and_increments() -> AppResult<()> {
    let e = env().await?;
    let emp = records::create(&e.db, fields("CEN-001", "Asha")).await?;
    let mut contract = fields("CEN-002", "Nimal");
    contract.employment_type = Some("CONTRACT".into());
    let contractor = records::create(&e.db, contract).await?;

    let first = salaries::add(
        &e.db,
        &emp.id,
        SalaryFields {
            effective_from: Some("2026-04-01".into()),
            basic: money(40_500),
            bra2005: money(1000),
            bra2016: money(2500),
            vehicle_allowance: money(15_000),
            other_allowance: money(24_850),
            ..Default::default()
        },
        &admin(),
    )
    .await?;
    assert_eq!(first["monthlyTotal"], 83_850);
    assert_eq!(first["epfBase"], 44_000);
    assert_eq!(first["welfareDeduction"], 300);
    assert_eq!(first["epfApplicable"], true);
    assert!(first["changeAmount"].is_null());

    let clash = salaries::add(
        &e.db,
        &emp.id,
        SalaryFields { effective_from: Some("2026-04-01".into()), basic: money(1), ..Default::default() },
        &admin(),
    )
    .await;
    assert!(
        matches!(clash, Err(AppError::BadRequest(ref m)) if matches!(m, ErrorMessage::Text(t) if t.contains("edit that one")))
    );

    let inc = salaries::add(
        &e.db,
        &emp.id,
        SalaryFields {
            effective_from: Some("2099-01-01".into()),
            basic: money(45_000),
            bra2005: money(1000),
            bra2016: money(2500),
            vehicle_allowance: money(15_000),
            other_allowance: money(24_850),
            ..Default::default()
        },
        &admin(),
    )
    .await?;
    assert_eq!(inc["changeAmount"], 4500);
    assert_eq!(inc["changePercent"], serde_json::json!(5.37));
    assert_eq!(inc["state"], "UPCOMING");

    let view = salaries::employee_payroll(&e.db, &emp.id, None).await?;
    assert_eq!(view["current"]["id"], first["id"]);
    assert_eq!(view["upcoming"]["id"], inc["id"]);
    assert_eq!(view["history"][0]["id"], inc["id"]);
    let later = salaries::employee_payroll(&e.db, &emp.id, Some("2099-02-01")).await?;
    assert_eq!(later["current"]["id"], inc["id"]);
    assert_eq!(later["history"][1]["state"], "PAST");
    assert!(later["upcoming"].is_null());

    let c = salaries::add(
        &e.db,
        &contractor.id,
        SalaryFields { effective_from: Some("2026-04-01".into()), basic: money(30_000), ..Default::default() },
        &admin(),
    )
    .await?;
    assert_eq!(c["epfApplicable"], false);
    assert_eq!(c["epfBase"], 0);

    let bank = salaries::update_bank(
        &e.db,
        &emp.id,
        BankFields {
            bank_name: Some(Some("Bank of Ceylon".into())),
            staff_loan_balance: Some(Money::from_cents(2_500_050)),
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(bank["staffLoanBalance"], serde_json::json!(25000.5));
    assert_eq!(bank["bank"]["bankName"], "Bank of Ceylon");

    let id = inc["id"].as_str().unwrap_or_default().to_string();
    let edited =
        salaries::update(&e.db, &emp.id, &id, SalaryFields { loan_installment: money(2500), ..Default::default() })
            .await?;
    assert_eq!(edited["loanInstallment"], 2500);
    salaries::remove(&e.db, &emp.id, &id).await?;
    assert!(matches!(salaries::remove(&e.db, &emp.id, &id).await, Err(AppError::NotFound(_))));
    Ok(())
}
