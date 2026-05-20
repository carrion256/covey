use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

mod support;

use covey::{
    ClaimNextReq, Covey, CreateSubtaskRequest, IdempotencyKey, ManualClock, RegisterSessionReq,
    ReleaseClaimReq, RequestReservationReq, ScopeClass, SessionRole, SubmitMetaTaskReq,
};
use proptest::prelude::*;
use tempfile::TempDir;

fn fresh_covey() -> (TempDir, Arc<ManualClock>, Covey) {
    support::enable_info_logging();
    let dir = TempDir::new().expect("tempdir");
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    let covey = Covey::open_with_clock(dir.path().join("covey.db"), clock.clone()).expect("covey");
    (dir, clock, covey)
}

fn id_key(label: &str) -> IdempotencyKey {
    static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
    IdempotencyKey::parse(format!(
        "{label}-{}",
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ))
    .expect("valid idempotency key")
}

fn seed_single_work_subtask(covey: &Covey) -> String {
    let orch = covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                "orch",
                "orch-1",
                SessionRole::Orchestrator,
                id_key("register-orch"),
            )
            .expect("valid session registration request"),
        )
        .expect("register orch")
        .session_token;
    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(orch.clone(), "meta", id_key("submit-meta"))
                .expect("valid submit-meta-task request"),
        )
        .expect("meta task");
    covey
        .create_subtask(CreateSubtaskRequest {
            session_token: covey::SessionToken::parse(orch.clone()).expect("valid session token"),
            meta_task_id: covey::MetaTaskId::parse(meta_task_id).expect("valid meta-task id"),
            subtask_id: Some(covey::SubtaskId::parse("work").expect("valid subtask id")),
            title: "work".into(),
            priority: covey::SubtaskPriority::parse(1).expect("valid subtask priority"),
            idempotency_key: id_key("create-subtask"),
        })
        .expect("create work");
    orch.to_string()
}

fn seed_two_work_subtasks(covey: &Covey) {
    let orch = covey
        .register_session(
            RegisterSessionReq::try_from_raw_parts(
                "orch",
                "orch-1",
                SessionRole::Orchestrator,
                id_key("register-orch"),
            )
            .expect("valid session registration request"),
        )
        .expect("register orch")
        .session_token;
    let meta_task_id = covey
        .submit_meta_task(
            SubmitMetaTaskReq::try_from_raw_parts(orch.clone(), "meta", id_key("submit-meta"))
                .expect("valid submit-meta-task request"),
        )
        .expect("meta task");
    for (subtask_id, priority) in [("work_a", 1_i64), ("work_b", 2_i64)] {
        covey
            .create_subtask(CreateSubtaskRequest {
                session_token: covey::SessionToken::parse(orch.clone())
                    .expect("valid session token"),
                meta_task_id: covey::MetaTaskId::parse(meta_task_id.clone())
                    .expect("valid meta-task id"),
                subtask_id: Some(covey::SubtaskId::parse(subtask_id).expect("valid subtask id")),
                title: subtask_id.into(),
                priority: covey::SubtaskPriority::parse(priority).expect("valid subtask priority"),
                idempotency_key: id_key("create-subtask"),
            })
            .expect("create work");
    }
}

proptest! {
    #[test]
    fn fence_sequences_increase_across_reclaims(reclaim_count in 1usize..12) {
        let (_dir, clock, covey) = fresh_covey();
        let _orch = seed_single_work_subtask(&covey);
        let worker = covey
            .register_session(
                RegisterSessionReq::try_from_raw_parts(
                    "worker",
                    "worker-1",
                    SessionRole::Executor,
                    id_key("register-worker"),
                )
                .expect("valid session registration request"),
            )
            .expect("register worker")
            .session_token;

        let mut seen = Vec::new();
        for _ in 0..reclaim_count {
            let claim = covey
                .claim_next_subtask(ClaimNextReq::try_from_raw_parts(
                    worker.clone(),
                    covey::LeaseDurationMs::parse(10_000).expect("valid lease duration"),
                    id_key("claim-next"),
                    ).expect("valid claim-next request"))
                .expect("claim call")
                .expect("claim result");
            seen.push(claim.fence_seq);
            covey
                .release_claim(ReleaseClaimReq::try_from_raw_parts(
                    worker.clone(),
                    claim.claim_id,
                    claim.fence_seq,
                    id_key("release-claim"),
                    ).expect("valid release-claim request"))
                .expect("release");
            clock.advance(1);
        }

        let mut sorted = seen.clone();
        sorted.sort_unstable();
        sorted.dedup();
        prop_assert_eq!(seen.len(), sorted.len());
    }

    #[test]
    fn overlap_rules_hold_for_subtree_exact_and_generated(
        base in "[a-z]{3,8}",
        child in "[a-z]{3,8}"
    ) {
        let (_dir, _clock, covey) = fresh_covey();
        let orch = seed_single_work_subtask(&covey);
        let reservation_id = covey
            .request_reservation(
                RequestReservationReq::try_from_raw_parts(
                    orch,
                    "work",
                    ScopeClass::Subtree,
                    format!("src/{base}"),
                    vec![],
                    60_000,
                    id_key("request-reservation"),
                )
                .expect("valid reservation request"),
            )
            .expect("reservation");

        let exact_overlaps = covey
            .find_overlapping_reservations(
                covey::OverlapQueryReq::try_from_parts(
                    ScopeClass::ExactPath,
                    format!("src/{base}/{child}.rs"),
                    vec![],
                )
                .expect("valid overlap query"),
            )
            .expect("overlaps");
        prop_assert_eq!(exact_overlaps.len(), 1);
        prop_assert_eq!(exact_overlaps[0].reservation_id.as_str(), reservation_id.as_str());

        let generated_overlaps = covey
            .find_overlapping_reservations(
                covey::OverlapQueryReq::try_from_parts(
                    ScopeClass::GeneratedSet,
                    format!("generated/{base}"),
                    vec![format!("src/{base}/{child}.rs")],
                )
                .expect("valid overlap query"),
            )
            .expect("generated overlaps");
        prop_assert_eq!(generated_overlaps.len(), 1);
    }

    #[test]
    fn fence_sequences_are_independent_per_subtask(reclaim_count in 1usize..12) {
        let (_dir, clock, covey) = fresh_covey();
        seed_two_work_subtasks(&covey);
        let worker_a = covey
            .register_session(
                RegisterSessionReq::try_from_raw_parts(
                    "worker_a",
                    "worker_a-1",
                    SessionRole::Executor,
                    id_key("register-worker"),
                )
                .expect("valid session registration request"),
            )
            .expect("register worker")
            .session_token;
        let worker_b = covey
            .register_session(
                RegisterSessionReq::try_from_raw_parts(
                    "worker_b",
                    "worker_b-1",
                    SessionRole::Executor,
                    id_key("register-worker"),
                )
                .expect("valid session registration request"),
            )
            .expect("register worker")
            .session_token;

        let mut seen = Vec::new();
        for _ in 0..reclaim_count {
            let claim = covey
                .claim_next_subtask(ClaimNextReq::try_from_raw_parts(
                    worker_a.clone(),
                    covey::LeaseDurationMs::parse(10_000).expect("valid lease duration"),
                    id_key("claim-next"),
                    ).expect("valid claim-next request"))
                .expect("claim call")
                .expect("claim result");
            prop_assert_eq!(claim.subtask_id, "work_a");
            seen.push(claim.fence_seq);
            covey
                .release_claim(ReleaseClaimReq::try_from_raw_parts(
                    worker_a.clone(),
                    claim.claim_id,
                    claim.fence_seq,
                    id_key("release-claim"),
                    ).expect("valid release-claim request"))
                .expect("release");
            clock.advance(1);
        }

        let held_a = covey
            .claim_next_subtask(ClaimNextReq::try_from_raw_parts(
                worker_a,
                covey::LeaseDurationMs::parse(10_000).expect("valid lease duration"),
                id_key("claim-next"),
                ).expect("valid claim-next request"))
            .expect("claim a")
            .expect("claim result");
        prop_assert_eq!(held_a.subtask_id, "work_a");

        let first_b = covey
            .claim_next_subtask(ClaimNextReq::try_from_raw_parts(
                worker_b,
                covey::LeaseDurationMs::parse(10_000).expect("valid lease duration"),
                id_key("claim-next"),
                ).expect("valid claim-next request"))
            .expect("claim b")
            .expect("claim result");
        prop_assert_eq!(first_b.subtask_id, "work_b");
        prop_assert_eq!(first_b.fence_seq, 1);

        let expected = (1..=reclaim_count as i64).collect::<Vec<_>>();
        prop_assert_eq!(seen, expected);
    }
}
