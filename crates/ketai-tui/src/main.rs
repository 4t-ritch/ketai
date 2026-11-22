mod view;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  view::render()?;
  Ok(())
}
