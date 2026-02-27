use std::collections::HashMap;
use std::fs::File;
use std::os::fd::AsRawFd;

struct Weather {
    samples: u32,
    min: i16,
    mean: i64,
    max: i16,
}

#[inline]
fn parse_temperature_inner(temperature: &[u8]) -> i16 {
    let to_digit = |b: u8| -> i16 { (b - b'0') as i16 };

    // Single digit temperature.
    if temperature[1] == b'.' {
        to_digit(temperature[0]) * 10 + to_digit(temperature[2])
    } else {
        to_digit(temperature[0]) * 100 + to_digit(temperature[1]) * 10 + to_digit(temperature[3])
    }
}

#[inline]
fn parse_temperature(temperature: &[u8]) -> i16 {
    if temperature[0] == b'-' {
        -parse_temperature_inner(&temperature[1..])
    } else {
        parse_temperature_inner(temperature)
    }
}

#[cfg(target_feature = "avx2")]
#[inline]
fn split_line(buf: &[u8]) -> (&[u8], &[u8], &[u8]) {
    use std::arch::x86_64::*;

    unsafe {
        let line: __m256i = _mm256_loadu_si256(buf.as_ptr() as *const __m256i);
        let sep: __m256i = _mm256_set1_epi8(b';' as i8);
        let eol: __m256i = _mm256_set1_epi8(b'\n' as i8);
        let sep_mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(line, sep));
        let eol_mask = _mm256_movemask_epi8(_mm256_cmpeq_epi8(line, eol));

        let sep_pos = sep_mask.trailing_zeros() as usize;
        let eol_pos = eol_mask.trailing_zeros() as usize;

        (
            &buf[..sep_pos],
            &buf[sep_pos + 1..eol_pos],
            &buf[eol_pos + 1..],
        )
    }
}

fn main() {
    let filename = std::env::args().nth(1).expect("missing filename");
    let file = File::open(filename).expect("could not open file");
    let mut buf = unsafe {
        let len = file.metadata().unwrap().len() as usize;
        let addr = libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            file.as_raw_fd(),
            0,
        );
        libc::madvise(addr, len, libc::MADV_SEQUENTIAL);
        std::slice::from_raw_parts(addr as *const u8, len)
    };
    let mut stats: HashMap<String, Weather> = HashMap::new();

    while !buf.is_empty() {
        if buf[0] == b'#' {
            continue;
        }
        let (city, temperature, remainder) = split_line(buf);
        buf = remainder;
        let temperature = parse_temperature(temperature);

        let city = unsafe { str::from_utf8_unchecked(city) };
        if let Some(entry) = stats.get_mut(city) {
            if temperature < entry.min {
                entry.min = temperature;
            } else if temperature > entry.max {
                entry.max = temperature;
            }

            entry.mean += temperature as i64;
            entry.samples += 1;
        } else {
            stats.insert(
                city.to_string(),
                Weather {
                    samples: 1,
                    min: temperature,
                    mean: temperature as i64,
                    max: temperature,
                },
            );
        }
    }

    let mut stats: Vec<(String, f64, f64, f64)> = stats
        .into_iter()
        .map(|(city, weather)| {
            (
                city,
                weather.min as f64 / 10.0,
                (weather.mean as f64 / 10.0 / weather.samples as f64),
                weather.max as f64 / 10.0,
            )
        })
        .collect();

    stats.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    print!("{{");
    let len = stats.len();
    for stat in stats.iter().take(len - 1) {
        print!("{}={:.1}/{:.1}/{:.1}, ", stat.0, stat.1, stat.2, stat.3,);
    }
    println!(
        "{}={:.1}/{:.1}/{:.1}}}",
        stats[len - 1].0,
        stats[len - 1].1,
        stats[len - 1].2,
        stats[len - 1].3
    );
}
