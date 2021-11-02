#[derive(Debug)]
pub struct Person {
    pub id: u64,
    pub name: String,
    pub age: u8,
    pub state: String,
}

impl Person {
    pub fn format_row(&self) -> String {
        format!(
            "{id:>8} | {name:16} | {age:>4} | {state:16}",
            id = self.id,
            name = self.name,
            age = self.age,
            state = self.state
        )
    }
}
