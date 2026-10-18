impl CollectorSession {

    fn record_metadata(&mut self, value: Value) {
        self.metadata = Some(match self.metadata.take() {
            None => value,
            Some(Value::Array(mut values)) => {
                values.push(value);
                Value::Array(values)
            }
            Some(existing) => Value::Array(vec![existing, value]),
        });
    }
}