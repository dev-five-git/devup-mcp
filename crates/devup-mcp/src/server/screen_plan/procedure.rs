// Where the workflow stands, said the same way in every plan reply.
//
// A weaker model does not hold a long skill in mind; it follows the reply it
// just read. So every devup_screen_plan and devup_project_plan reply, and
// every export the gate refuses, carries one `procedure` block computed from
// the plan on disk: the step the project is at, the one call to make next,
// whether it must stop and wait for the user, and what is not allowed now.
// The block is derived, never stored, so it cannot drift from the plan.

/// The procedure block for a project, from its screen plan on disk.
pub(crate) fn procedure(project_root: &str) -> Value {
    let plan = read_plan(project_root).ok().flatten();
    let step = |index: usize, id: &str, next: Value, wait: bool, forbidden: &[&str]| {
        json!({
            "step": format!("{index}/5 {id}"),
            "steps": ["1 draft", "2 ask-user", "3 confirm-with-approval", "4 implement", "5 verify"],
            "next": next,
            "waitForUser": wait,
            "forbiddenNow": forbidden,
            "rule": "Follow `next` exactly. Do not skip a step, do not answer for the user, do not claim completion before status says completion.reportAllowed.",
        })
    };
    let Some(plan) = plan else {
        return step(1, "draft",
            json!({"tool": "devup_screen_plan", "arguments": {"action": "draft", "projectRoot": project_root},
                "how": "Draft the screen plan from the Figma link (or the Bridge selection) before anything else."}),
            false,
            &["writing or exporting pages", "hand-writing TSX", "confirming anything"]);
    };
    let plan_id = plan["planId"].clone();
    if plan["status"] != "confirmed" {
        return step(2, "ask-user",
            json!({"tool": "question-tool",
                "how": "Show proposalMarkdown (every screen, its URL and each open decision with the recommended answer) in the user's language and ask ONE question with the runtime question tool. Then STOP and wait for the answer. Only after it: devup_screen_plan confirm with userApproval.",
                "then": {"tool": "devup_screen_plan", "arguments": {"action": "confirm", "projectRoot": project_root, "planId": plan_id,
                    "acceptRecommended": true, "answers": "<only the rows the user changed>",
                    "userApproval": {"via": "question-tool", "userAnswer": "<the user's answer, verbatim>", "approvedUrls": "<every URL in the table shown>"}}}}),
            true,
            &["confirm in the same turn as the question", "confirm without the user's answer", "exporting screens or writing pages", "hand-writing TSX"]);
    }
    if !has_user_approval(&plan) {
        return step(3, "confirm-with-approval",
            json!({"tool": "devup_screen_plan", "arguments": {"action": "confirm", "projectRoot": project_root, "planId": plan_id, "answers": {},
                    "userApproval": {"via": "question-tool", "userAnswer": "<the user's answer, verbatim>", "approvedUrls": plan_urls(&plan)}},
                "how": "The plan is confirmed but the user never approved its URLs. Show markdownTable, ask with the question tool, wait, then send this call with the user's answer."}),
            true,
            &["exporting screens or writing pages", "hand-writing TSX"]);
    }
    let audited = plan["audit"].is_object();
    if !audited {
        return step(4, "implement",
            json!({"tool": "devup_screen_plan", "arguments": {"action": "status", "projectRoot": project_root, "planId": plan_id},
                "how": "Build every URL in workQueue with devup_figma_export (components first, then sections), validate with devup_ui_validate, then auditPlan and the runtime audit. New decisions go to defer/assume, never to the user."}),
            false,
            &["asking the user (use defer or assume)", "hand-writing TSX from Figma nodes", "claiming completion"]);
    }
    step(5, "verify",
        json!({"tool": "devup_screen_plan", "arguments": {"action": "status", "projectRoot": project_root, "planId": plan_id},
            "how": "Run the runtime audit for every URL, pass auditPlanPath/auditReportPath to status, fix findings in workQueue order and re-run. Report completion only when completion.reportAllowed is true."}),
        false,
        &["claiming completion before completion.reportAllowed", "asking the user (use defer or assume)"])
}
