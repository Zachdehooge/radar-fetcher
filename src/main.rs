use std::io;
use std::io::Write;

fn prompt_int(prompt: &str) -> i32 {
    loop {
        print!("{}", prompt);
        io::stdout().flush().expect("Failed to flush stdout");

        let mut input = String::new();
        io::stdin().read_line(&mut input).expect("Failed to read input");

        match input.trim().parse::<i32>() {
            Ok(num) => return num,
            Err(_) => {
                println!("Please enter a valid integer.");
            }
        }
    }
}
fn prompt_input(prompt: &str) -> String {
    print!("{}", prompt);
    io::stdout().flush().expect("Failed to flush stdout");

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read input");
    input.trim().to_string()
}

fn main() {
    let month_start = prompt_int("Enter month(01): ");
    let month_start = format!("{:02}", month_start);
    let day_start = prompt_int("Enter day(01): ");
    let day_start = format!("{:02}", day_start);
    let year_start = prompt_int("Enter starting year: ");

    let radar = prompt_input("Enter radar: ");

    let url = "https://www.ncdc.noaa.gov/nexradinv/bdp-download.jsp?id=".to_owned() + &*radar.to_uppercase() + "&yyyy=" + &*year_start.to_string() + "&mm=" + &*month_start.to_string() + "&dd=" + &*day_start.to_string() + "&product=AAL2";
    println!("Url: {:?}", url);
}
