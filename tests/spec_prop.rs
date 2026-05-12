use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

mod support;

use covey::{
    ClaimNextReq, Covey, CreateSubtaskReq, ManualClock, RegisterSessionReq, ReleaseClaimReq,
    RequestReservationReq, ScopeClass, SessionRole, SubmitMetaTaskReq, SubtaskKind,
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

fn id_key(label: &str) -> String {
    static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
    format!("{label}-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

fn seed_single_work_subtask(covey: &Covey) -> String {
    let orch = covey
        .register_session(RegisterSessionReq {
            agent_principal_id: "orch".into(),
            agent_instance_id: "orch-1".into(),
            role: SessionRole::Orchestrator,
            idempotency_key: id_key("register-orch"),
        })
        .expect("register orch")
        .session_token;
    let meta_task_id = covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orch.clone(),
            prompt_text: "meta".into(),
            idempotency_key: id_key("submit-meta"),
        })
        .expect("meta task");
    covey
        .create_subtask(CreateSubtaskReq {
            session_token: orch.clone(),
            meta_task_id,
            subtask_id: Some("work".into()),
            title: "work".into(),
            kind: SubtaskKind::Work,
            review_target_subtask_id: None,
            review_target_artifact_digest: None,
            priority: 1,
            idempotency_key: id_key("create-subtask"),
        })
        .expect("create work");
    orch
}

fn seed_two_work_subtasks(covey: &Covey) {
    let orch = covey
        .register_session(RegisterSessionReq {
            agent_principal_id: "orch".into(),
            agent_instance_id: "orch-1".into(),
            role: SessionRole::Orchestrator,
            idempotency_key: id_key("register-orch"),
        })
        .expect("register orch")
        .session_token;
    let meta_task_id = covey
        .submit_meta_task(SubmitMetaTaskReq {
            session_token: orch.clone(),
            prompt_text: "meta".into(),
            idempotency_key: id_key("submit-meta"),
        })
        .expect("meta task");
    for (subtask_id, priority) in [("work_a", 1_i64), ("work_b", 2_i64)] {
        covey
            .create_subtask(CreateSubtaskReq {
                session_token: orch.clone(),
                meta_task_id: meta_task_id.clone(),
                subtask_id: Some(subtask_id.into()),
                title: subtask_id.into(),
                kind: SubtaskKind::Work,
                review_target_subtask_id: None,
                review_target_artifact_digest: None,
                priority,
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
            .register_session(RegisterSessionReq {
                agent_principal_id: "worker".into(),
                agent_instance_id: "worker-1".into(),
                role: SessionRole::Executor,
                idempotency_key: id_key("register-worker"),
            })
            .expect("register worker")
            .session_token;

        let mut seen = Vec::new();
        for _ in 0..reclaim_count {
            let claim = covey
                .claim_next_subtask(ClaimNextReq {
                    session_token: worker.clone(),
                    lease_duration_ms: 10_000,
                    idempotency_key: id_key("claim-next"),
                })
                .expect("claim call")
                .expect("claim result");
            seen.push(claim.fence_seq);
            covey
                .release_claim(ReleaseClaimReq {
                    session_token: worker.clone(),
                    claim_id: claim.claim_id,
                    fence_seq: claim.fence_seq,
                    idempotency_key: id_key("release-claim"),
                })
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
            .request_reservation(RequestReservationReq {
                session_token: orch,
                owner_subtask_id: "work".into(),
                scope_class: ScopeClass::Subtree,
                scope_key: format!("src/{base}"),
                generated_members: vec![],
                lease_duration_ms: 60_000,
                idempotency_key: id_key("request-reservation"),
            })
            .expect("reservation");

        let exact_overlaps = covey
            .find_overlapping_reservations(covey::OverlapQueryReq {
                scope_class: ScopeClass::ExactPath,
                scope_key: format!("src/{base}/{child}.rs"),
                generated_members: vec![],
            })
            .expect("overlaps");
        prop_assert_eq!(exact_overlaps.len(), 1);
        prop_assert_eq!(exact_overlaps[0].reservation_id.as_str(), reservation_id.as_str());

        let generated_overlaps = covey
            .find_overlapping_reservations(covey::OverlapQueryReq {
                scope_class: ScopeClass::GeneratedSet,
                scope_key: format!("generated/{base}"),
                generated_members: vec![format!("src/{base}/{child}.rs")],
            })
            .expect("generated overlaps");
        prop_assert_eq!(generated_overlaps.len(), 1);
    }

    #[test]
    fn fence_sequences_are_independent_per_subtask(reclaim_count in 1usize..12) {
        let (_dir, clock, covey) = fresh_covey();
        seed_two_work_subtasks(&covey);
        let worker_a = covey
            .register_session(RegisterSessionReq {
                agent_principal_id: "worker_a".into(),
                agent_instance_id: "worker_a-1".into(),
                role: SessionRole::Executor,
                idempotency_key: id_key("register-worker"),
            })
            .expect("register worker")
            .session_token;
        let worker_b = covey
            .register_session(RegisterSessionReq {
                agent_principal_id: "worker_b".into(),
                agent_instance_id: "worker_b-1".into(),
                role: SessionRole::Executor,
                idempotency_key: id_key("register-worker"),
            })
            .expect("register worker")
            .session_token;

        let mut seen = Vec::new();
        for _ in 0..reclaim_count {
            let claim = covey
                .claim_next_subtask(ClaimNextReq {
                    session_token: worker_a.clone(),
                    lease_duration_ms: 10_000,
                    idempotency_key: id_key("claim-next"),
                })
                .expect("claim call")
                .expect("claim result");
            prop_assert_eq!(claim.subtask_id, "work_a");
            seen.push(claim.fence_seq);
            covey
                .release_claim(ReleaseClaimReq {
                    session_token: worker_a.clone(),
                    claim_id: claim.claim_id,
                    fence_seq: claim.fence_seq,
                    idempotency_key: id_key("release-claim"),
                })
                .expect("release");
            clock.advance(1);
        }

        let held_a = covey
            .claim_next_subtask(ClaimNextReq {
                session_token: worker_a,
                lease_duration_ms: 10_000,
                idempotency_key: id_key("claim-next"),
            })
            .expect("claim a")
            .expect("claim result");
        prop_assert_eq!(held_a.subtask_id, "work_a");

        let first_b = covey
            .claim_next_subtask(ClaimNextReq {
                session_token: worker_b,
                lease_duration_ms: 10_000,
                idempotency_key: id_key("claim-next"),
            })
            .expect("claim b")
            .expect("claim result");
        prop_assert_eq!(first_b.subtask_id, "work_b");
        prop_assert_eq!(first_b.fence_seq, 1);

        let expected = (1..=reclaim_count as i64).collect::<Vec<_>>();
        prop_assert_eq!(seen, expected);
    }
}
