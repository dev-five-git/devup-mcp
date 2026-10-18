impl CollectorSession {

    fn enqueue(&mut self, call: ReadToolCall, expected_node_id: Option<String>, kind: CallKind) {
        let order = self.next_id;
        let id = format!("call-{order}");
        self.next_id += 1;
        self.queued.push_back(PendingCall {
            planned: PlannedCall {
                id,
                call,
                expected_file_key: self.request.target.file_key.clone(),
                expected_node_id,
            },
            kind,
            order,
        });
    }
}