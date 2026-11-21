mod decoder;
mod ui;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  ui::view::render()?;
  Ok(())
}
