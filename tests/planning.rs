use grok_search_rs::planning::PlanningEngine;

#[test]
fn planning_session_accumulates_required_phases() {
    let mut engine = PlanningEngine::default();
    let intent = engine.plan_intent("", "Need current OpenAI news", "factual", "recent", 0.9);
    let session_id = intent.session_id;

    let complexity = engine.plan_complexity(&session_id, 1, 1, 1, "single factual lookup", 0.9);
    assert_eq!(complexity.complexity_level, Some(1));

    let result = engine.plan_sub_query(
        &session_id,
        "sq1",
        "Find current official announcement",
        "one sourced answer",
        "excludes rumors",
        0.9,
    );

    assert!(result.plan_complete);
    assert!(result.executable_plan.is_some());
}

#[test]
fn plan_search_builds_complete_plan_in_one_call() {
    let mut engine = PlanningEngine::default();

    let result = engine.plan_search("2026年5月14日 OpenAI 最新官方公告", "auto", "recent", 0.9);

    assert!(result.plan_complete);
    let plan = result.executable_plan.expect("executable plan");
    assert!(plan.get("intent_analysis").is_some());
    assert!(plan.get("complexity_assessment").is_some());
    assert!(plan.get("query_decomposition").is_some());
    assert!(plan.get("search_strategy").is_some());
    assert!(plan.get("tool_selection").is_some());
    assert!(plan.get("execution_order").is_some());
}
