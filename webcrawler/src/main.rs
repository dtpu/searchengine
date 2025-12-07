mod parser;
use ureq;

fn main() {
    let seeds = vec![
        "https://example.com",
        "https://student.cs.uwaterloo.ca/~cs145/",
    ];
    
    for seed in seeds {
        let response = ureq::get(seed).call().expect("Failed to fetch URL");
        let html_content = response.into_body().read_to_string().expect("Failed to read response body");
        let parsed = parser::parse_html(html_content, seed);
        println!("Parsed links from {}: {:?}", seed, parsed.links);
    }
}