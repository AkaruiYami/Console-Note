mod iomodule;

fn main() {
    let msg = iomodule::read();
    println!("{:?}", msg);
}
