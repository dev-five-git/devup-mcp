/// A PAGE source URL with several frameIds is not a Section selection.
/// Anchor on an already verified requested frame so collection can resolve
/// its containing Section. Keep the real file/branch identity and all other
/// query pairs; no file key or ancestor node ID is invented.
fn anchor_export_arguments(plan: &Value, arguments: &mut Value) {
    let Some(source) = plan["source"]["url"].as_str() else {
        return;
    };
    arguments["url"] = json!(source);
    let Some(node) = array(&arguments["frameIds"])
        .first()
        .and_then(Value::as_str)
    else {
        return;
    };
    let Ok(mut url) = url::Url::parse(source) else {
        return;
    };
    let pairs = url
        .query_pairs()
        .filter(|(key, _)| key != "node-id")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    let node = node.replace(':', "-");
    url.set_query(None);
    url.query_pairs_mut()
        .extend_pairs(pairs)
        .append_pair("node-id", &node);
    arguments["url"] = json!(url.to_string());
}

fn group_export_action(plan: &Value, group: &Value, root: &str) -> Value {
    let widths = array(&group["drawnWidths"]);
    let mut frames = array(&group["frameIds"])
        .iter()
        .enumerate()
        .collect::<Vec<_>>();
    frames.sort_by_key(|(index, _)| {
        widths
            .get(*index)
            .and_then(Value::as_u64)
            .unwrap_or_default()
    });
    let output = if frames.len() > 1 {
        "responsiveTsx"
    } else {
        "tsx"
    };
    let mut arguments = json!({"projectRoot":root,"frameIds":frames.iter().map(|(_, id)| *id).collect::<Vec<_>>(),"outputs":[output]});
    anchor_export_arguments(plan, &mut arguments);
    json!({"tool":"devup_figma_export","arguments":arguments,"groupId":group["id"],"groupKind":group["kind"]})
}

fn responsive_grouping_recovery(plan: &Value, route: &Value, root: &str, reason: &str) -> Value {
    let reads = array(&route["nodeIds"])
        .chunks(3)
        .map(|ids| {
            let mut arguments = json!({"projectRoot":root,"frameIds":ids,"outputs":["outline"]});
            anchor_export_arguments(plan, &mut arguments);
            json!({"tool":"devup_figma_export","arguments":arguments})
        })
        .collect::<Vec<_>>();
    json!({"status":"responsive_grouping_required","reason":reason,
        "tool":reads.first().map(|read| read["tool"].clone()),"arguments":reads.first().map(|read| read["arguments"].clone()),
        "remainingReads":reads.iter().skip(1).collect::<Vec<_>>(),
        "candidates":array(&route["nodeIds"]).iter().enumerate().map(|(index,id)| {
            let frame = array(&plan["screens"]).iter().flat_map(|screen| array(&screen["frames"]))
                .find(|frame| frame["nodeId"] == *id);
            json!({"nodeId":id,"width":array(&route["drawnWidths"]).get(index),"name":frame.map(|frame| &frame["name"])})
        }).collect::<Vec<_>>(),
        "resolution":{"tool":"devup_screen_plan","action":"assume","projectRoot":root,"planId":plan["planId"],
            "field":"responsiveGroups", "shape":{"url":route["url"],"id":"source-evidenced-state-name","kind":"baseline or state","frameIds":"one actual frame per width","reason":"the source evidence proving this pairing"},
            "requirements":"Record every route frame exactly once, one baseline group and named state groups, at most six frames per group. Never choose a winner from duplicate names or frame order."}})
}

pub(super) fn responsive_export_action(plan: &Value, route: &Value, root: &str) -> Value {
    match responsive_groups(route) {
        Ok(groups) => {
            let mut action = group_export_action(plan, &groups[0], root);
            action["remainingActions"] = json!(
                groups
                    .iter()
                    .skip(1)
                    .map(|group| group_export_action(plan, group, root))
                    .collect::<Vec<_>>()
            );
            action
        }
        Err(reason) => responsive_grouping_recovery(plan, route, root, &reason),
    }
}

fn multi_width(route: &Value) -> bool {
    array(&route["drawnWidths"])
        .iter()
        .map(Value::to_string)
        .collect::<BTreeSet<_>>()
        .len()
        > 1
        && array(&route["nodeIds"]).len() > 1
        && route["implementation"] != "state"
}

pub(super) fn responsive_required(
    root: &str,
    frames: &[String],
    outputs: &[String],
) -> Result<Option<Value>, DevupError> {
    let responsive = outputs.iter().any(|output| output == "responsiveTsx");
    if !outputs.iter().any(|output| {
        matches!(
            output.as_str(),
            "tsx" | "componentTsx" | "pageScaffold" | "responsiveTsx"
        )
    }) {
        return Ok(None);
    }
    for ancestor in Path::new(root).ancestors() {
        let Some(plan) = read_plan(&ancestor.to_string_lossy())? else {
            continue;
        };
        if plan["status"] != "confirmed" {
            continue;
        }
        let route = array(&plan["routes"]).iter().find(|route| {
            multi_width(route)
                && frames.iter().any(|id| {
                    plan["source"]["nodeId"] == id.as_str()
                        || array(&route["nodeIds"]).contains(&json!(id))
                        || array(&plan["frameInventory"]["frames"])
                            .iter()
                            .any(|frame| {
                                frame["nodeId"] == id.as_str()
                                    && array(&frame["ancestors"])
                                        .iter()
                                        .any(|parent| array(&route["nodeIds"]).contains(parent))
                            })
                })
        });
        if let Some(route) = route {
            let groups = match responsive_groups(route) {
                Ok(groups) => groups,
                Err(reason) => {
                    return Ok(Some(
                        json!({"status":"responsive_grouping_required","written":false,
                    "responsiveRequired":{"code":"DEVUP_RESPONSIVE_GROUPING_REQUIRED","url":route["url"],"reason":reason},
                    "nextAction":responsive_grouping_recovery(&plan,route,root,&reason)}),
                    ));
                }
            };
            let selected = groups.iter().find(|group| {
                frames
                    .iter()
                    .all(|frame| array(&group["frameIds"]).contains(&json!(frame)))
            });
            if let Some(group) = selected {
                let complete = array(&group["frameIds"])
                    .iter()
                    .all(|id| frames.iter().any(|frame| id == frame));
                let valid_output = if array(&group["frameIds"]).len() == 1 {
                    !outputs.iter().any(|output| output == "responsiveTsx")
                } else {
                    responsive
                        && !outputs
                            .iter()
                            .any(|output| output == "componentTsx")
                };
                if complete && valid_output {
                    return Ok(None);
                }
            }
            let group = selected.unwrap_or(&groups[0]);
            return Ok(Some(json!({"status":"responsive_required","written":false,
                "responsiveRequired":{"code":"DEVUP_RESPONSIVE_REQUIRED","url":route["url"],"frameIds":group["frameIds"],
                    "reason":"Export one recorded baseline/state group at a time. Multi-width groups require responsiveTsx; generated layout values must remain unchanged."},
                "nextAction":group_export_action(&plan,group,root),
                "groupActions":groups.iter().map(|group| group_export_action(&plan,group,root)).collect::<Vec<_>>()})));
        }
        break;
    }
    Ok(None)
}

fn responsive_status(
    plan: &Value,
    route: &Value,
    evidence: &ProjectEvidence,
    records: &[Value],
) -> Value {
    let ids = array(&route["nodeIds"]);
    let matching = records
        .iter()
        .filter(|record| {
            array(&record["frameIds"]).iter().any(|id| ids.contains(id))
                || ids.contains(&record["rootNodeId"])
        })
        .collect::<Vec<_>>();
    let groups = match responsive_groups(route) {
        Ok(groups) => groups,
        Err(reason) => {
            return json!({"required":multi_width(route),"recorded":!matching.is_empty(),"responsive":false,
            "issue":reason,"nextAction":responsive_grouping_recovery(plan,route,&evidence.root.to_string_lossy(),&reason),"groups":[],"generatedDrift":null});
        }
    };
    let sources = evidence
        .route_sources
        .get(text(&route["url"]))
        .cloned()
        .unwrap_or_default();
    let rows = groups.iter().map(|group| {
        let ids = array(&group["frameIds"]);
        let record = matching.iter().find(|record| {
            (if ids.len() > 1 { record["output"] == "responsiveTsx" } else { matches!(text(&record["output"]), "tsx" | "componentTsx") })
                && ids.iter().all(|id| array(&record["frameIds"]).contains(id) || record["rootNodeId"] == *id)
                && array(&record["frameIds"]).iter().all(|id| ids.contains(id))
        });
        json!({"id":group["id"],"kind":group["kind"],"frameIds":ids,"responsive":record.is_some(),"reason":group["reason"],
            "nextAction":group_export_action(plan,group,&evidence.root.to_string_lossy()),
            "generatedDrift":record.map(|record| super::export_record::generated_drift(record,&evidence.root,&sources))})
    }).collect::<Vec<_>>();
    let complete = rows.iter().all(|group| group["responsive"] == true);
    let next = rows
        .iter()
        .find(|group| group["responsive"] != true)
        .unwrap_or(&rows[0]);
    json!({"required":multi_width(route),"recorded":!matching.is_empty(),"responsive":complete,
        "issue":if multi_width(route) && !complete && !matching.is_empty() { Some("baseline/state responsive export groups are missing or use single-width code") } else { None },
        "nextAction":if multi_width(route) { next["nextAction"].clone() } else { Value::Null },
        "generatedDrift":rows[0]["generatedDrift"],"groups":rows})
}
