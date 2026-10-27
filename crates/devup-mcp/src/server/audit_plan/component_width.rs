impl Sources {
    fn effective(&self, frames: &[(u32, String, Option<f64>)]) -> (Self, Vec<Value>) {
        let Some((_, first)) = self.payloads.first() else {
            return (Self { payloads: Vec::new(), substitutions: Vec::new() }, Vec::new());
        };
        let mut snapshot = first.snapshot.clone();
        for (_, payload) in &self.payloads {
            snapshot.nodes.extend(payload.snapshot.nodes.clone());
        }
        let roots = snapshot.nodes.iter().filter(|(_, node)| node.node_type == "FRAME")
            .filter(|(id, _)| !snapshot.nodes.values().any(|parent| parent.typed_view().child_ids().any(|child| child == id.as_str())))
            .map(|(id, _)| id.clone()).collect::<Vec<_>>();
        let substitutions = if self.substitutions.is_empty() {
            super::screen_plan::component_width_mismatches(&snapshot, &roots)
        } else { self.substitutions.clone() }.into_iter().filter(|row| frames.iter().any(|(_,id,_)|row["screen"] == *id)).collect::<Vec<_>>();
        let mut payloads = Vec::new();
        for (artifact, original) in &self.payloads {
            let mut payload = original.as_ref().clone();
            for substitution in &substitutions {
                let (Some(suspect), Some(correct), Some(screen)) = (substitution["nodeId"].as_str(),substitution["correctNodeId"].as_str(),substitution["screen"].as_str()) else { continue };
                if !payload.snapshot.nodes.contains_key(suspect) { continue; }
                let Some(source) = self.payloads.iter().map(|(_,payload)|payload.as_ref()).find(|payload|payload.snapshot.nodes.contains_key(correct)) else { continue };
                let (Some(old_box),Some(new_box),Some(frame_box)) = (bounds(&payload,suspect),bounds(source,correct),bounds(&payload,screen)) else { continue };
                let mut stack = vec![correct.to_owned()];
                let mut seen = BTreeSet::new();
                while let Some(id) = stack.pop() {
                    if !seen.insert(id.clone()) { continue; }
                    let Some(node) = source.snapshot.nodes.get(&id) else { continue };
                    stack.extend(node.typed_view().child_ids().map(str::to_owned));
                    let mut replacement = node.clone();
                    if let Some(position) = bounds(source,&id) {
                        replacement.fields.insert("absoluteBoundingBox".into(),json!({
                            "x":frame_box.x + position.x - new_box.x,"y":old_box.y + position.y - new_box.y,
                            "width":if id == correct {frame_box.width} else {position.width},"height":position.height}));
                    }
                    let destination = if id == correct {suspect} else {&id};
                    replacement.id = destination.into();
                    payload.snapshot.nodes.insert(destination.into(),replacement);
                }
            }
            payloads.push((artifact.clone(),Arc::new(payload)));
        }
        (Self {payloads,substitutions:substitutions.clone()},substitutions)
    }
}
