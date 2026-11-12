// The user's approval, as a receipt the confirm call must carry.
//
// `confirm` used to take `acceptRecommended: true` on its own. An agent that
// skipped showing the URL table - or never asked at all - confirmed the plan
// itself, and pages were built on URLs nobody had seen. Prose in the skill
// did not stop that: a weaker model read "confirm" in nextAction and called
// it. So the confirm that a client sends through the tool now needs
// `userApproval`: how the user answered, the user's own words, and (for the
// screen plan) every URL of the table exactly as the user saw it. A receipt
// whose URLs differ from what confirm would build is refused with the table
// to show, so the user always approves the URLs that are built. The receipt
// is written into the plan and reported by status and the final report.

/// The user's approval of a plan, sent with `confirm`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties" = json!({"not": {}})))]
pub struct UserApprovalInput {
    /// How the user approved. `question-tool`: the user chose an answer in the
    /// runtime's question tool after the table was shown. `user-message`: the
    /// user typed the approval in chat after the table was shown. `delegated`:
    /// earlier in this session the user explicitly told you to decide without
    /// asking - quote those words in userAnswer; the plan is then reported as
    /// not reviewed by the user.
    #[schemars(extend("enum" = ["question-tool", "user-message", "delegated"]))]
    pub via: String,
    /// The user's own answer, verbatim: the option label they chose or the
    /// text they typed. Never your summary, never a recommendation they did
    /// not pick.
    pub user_answer: String,
    /// devup_screen_plan: every URL in the table the user approved, exactly
    /// as shown (e.g. ["/", "/about"]). It must equal the URLs this confirm
    /// builds; when it does not, confirm refuses and returns the table to
    /// show the user.
    #[serde(default)]
    pub approved_urls: Vec<String>,
}

const APPROVAL_VIA: [&str; 3] = ["question-tool", "user-message", "delegated"];

/// What a client must do when confirm has no valid approval.
pub(super) const APPROVAL_INSTRUCTION: &str = "Stop. Do not confirm on your own. Show the user the table (markdownTable: every screen and its URL) in the language they write in, ask with the runtime question tool (accept, or name the rows to change), wait for the answer, then call confirm again with userApproval: {via: \"question-tool\", userAnswer: \"<the label the user chose, verbatim>\", approvedUrls: [<every URL in the table, exactly>]}. Only when the user already told you in this session to decide without asking, use via \"delegated\" and quote their words in userAnswer.";

fn approval_error(message: String, details: Value) -> DevupError {
    let mut details = details;
    details["approvalRequired"] = json!(true);
    details["how"] = json!(APPROVAL_INSTRUCTION);
    DevupError::with_details(ErrorCode::DevupInvalidInput, message, false, details)
}

/// The receipt, checked for shape: a known `via` and the user's own words.
/// `required` is set for a confirm sent through the tool; `None` then fails.
pub(crate) fn approval_receipt<'a>(
    approval: Option<&'a UserApprovalInput>,
    required: bool,
    what: &str,
    table: &Value,
    expected_urls: &[String],
) -> Result<Option<&'a UserApprovalInput>, DevupError> {
    let Some(approval) = approval else {
        if required {
            return Err(approval_error(
                format!("confirm needs userApproval: the user has to approve the {what} before it is confirmed, and this call carries no approval."),
                json!({"missing": "userApproval", "markdownTable": table, "expectedUrls": expected_urls}),
            ));
        }
        return Ok(None);
    };
    if !APPROVAL_VIA.contains(&approval.via.as_str()) {
        return Err(approval_error(
            format!("userApproval.via must be one of {}, not {:?}.", APPROVAL_VIA.join(", "), approval.via),
            json!({"markdownTable": table}),
        ));
    }
    let answer = approval.user_answer.trim();
    if answer.is_empty() || (answer.contains('<') && answer.contains('>')) {
        return Err(approval_error(
            "userApproval.userAnswer must be the user's own answer, verbatim, not empty and not a placeholder.".into(),
            json!({"markdownTable": table}),
        ));
    }
    Ok(Some(approval))
}

/// The approved URLs against the URLs confirm builds; refused with both
/// lists and the table when they differ.
pub(crate) fn check_approved_urls(
    approval: &UserApprovalInput,
    built: &[String],
    table: &Value,
) -> Result<(), DevupError> {
    let normalize = |url: &str| {
        let url = url.trim();
        if url.len() > 1 { url.trim_end_matches('/').to_owned() } else { url.to_owned() }
    };
    let approved = approval.approved_urls.iter().map(|url| normalize(url)).collect::<BTreeSet<_>>();
    let expected = built.iter().map(|url| normalize(url)).collect::<BTreeSet<_>>();
    if approved == expected {
        return Ok(());
    }
    let missing = expected.difference(&approved).cloned().collect::<Vec<_>>();
    let unexpected = approved.difference(&expected).cloned().collect::<Vec<_>>();
    Err(approval_error(
        format!(
            "userApproval.approvedUrls does not match the URLs this confirm would build. Not approved: {}. Approved but not in the plan: {}. Nothing was confirmed: show this table to the user and ask again.",
            if missing.is_empty() { "none".to_owned() } else { missing.join(", ") },
            if unexpected.is_empty() { "none".to_owned() } else { unexpected.join(", ") },
        ),
        json!({"expectedUrls": expected, "notApproved": missing, "notInPlan": unexpected, "markdownTable": table}),
    ))
}

/// The receipt as the plan records it.
pub(crate) fn approval_record(approval: Option<&UserApprovalInput>) -> Value {
    match approval {
        Some(approval) => json!({
            "via": approval.via,
            "userAnswer": approval.user_answer.trim(),
            "approvedUrls": approval.approved_urls,
            "reviewedByUser": approval.via != "delegated",
            "recordedAt": now(),
        }),
        None => json!({"via": "none", "reviewedByUser": false,
            "note": "Confirmed without a user approval receipt (not through the devup_screen_plan tool)."}),
    }
}

/// One line for status and the final report.
pub(crate) fn approval_line(record: &Value) -> String {
    match record["via"].as_str() {
        Some("question-tool") | Some("user-message") => format!(
            "approved by the user ({}): \"{}\"",
            text(&record["via"]),
            text(&record["userAnswer"])
        ),
        Some("delegated") => format!(
            "NOT reviewed by the user: decided by the agent on the user's instruction \"{}\"",
            text(&record["userAnswer"])
        ),
        _ => "NOT reviewed by the user: no approval receipt was recorded".to_owned(),
    }
}

/// Every URL a plan builds, sorted and unique.
pub(crate) fn plan_urls(plan: &Value) -> Vec<String> {
    array(&plan["routes"])
        .iter()
        .filter_map(|route| route["url"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Whether the plan records the user's approval: their own answer, or their
/// explicit instruction to decide alone. No receipt, or one recorded without
/// the user, is not approval.
pub(crate) fn has_user_approval(plan: &Value) -> bool {
    matches!(
        plan["userApproval"]["via"].as_str(),
        Some("question-tool" | "user-message" | "delegated")
    )
}

/// The export gate's refusal for a confirmed plan the user never approved.
pub(super) fn approval_gate(plan: &Value, project_root: &str) -> Value {
    let urls = plan_urls(plan);
    json!({
        "status": "approval_required",
        "written": false,
        "approvalRequired": {
            "reason": "The screen plan is confirmed, but no user approval of its URLs is recorded. No page is written until the user has seen the screens and URLs table and approved it.",
            "planId": plan["planId"],
            "markdownTable": route_markdown(array(&plan["routes"])),
            "expectedUrls": urls,
        },
        "nextAction": {"tool": "devup_screen_plan",
            "arguments": {"action": "confirm", "projectRoot": project_root, "planId": plan["planId"], "answers": {},
                "userApproval": {"via": "question-tool", "userAnswer": "<the label the user chose, verbatim>", "approvedUrls": urls}},
            "how": APPROVAL_INSTRUCTION, "mustAskUser": true},
    })
}
