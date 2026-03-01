use hashbrown::HashMap;
use std::collections::BTreeMap;
use std::fs::File;
use std::hash::Hash;
use std::os::fd::AsRawFd;

#[derive(Eq)]
struct Station<'a>(&'a [u8]);

struct Weather {
    samples: u32,
    min: i16,
    mean: i64,
    max: i16,
}

impl<'a> Hash for Station<'a> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<'a> PartialEq for Station<'a> {
    #[cfg(target_feature = "avx2")]
    fn eq(&self, other: &Self) -> bool {
        use std::arch::x86_64::*;

        if self.0.len() != other.0.len() {
            return false;
        }
        let diff = unsafe {
            let lhs: __m256i = _mm256_loadu_si256(self.0.as_ptr() as *const __m256i);
            let rhs: __m256i = _mm256_loadu_si256(other.0.as_ptr() as *const __m256i);
            _mm256_movemask_epi8(_mm256_cmpeq_epi8(lhs, rhs)) as u32
        };

        let mask = (1 << self.0.len()) - 1;

        diff & mask == mask
    }
}

fn get_byte(bytes: &[u8], position: usize) -> u8 {
    unsafe { *bytes.get_unchecked(position) }
}

fn parse_temperature_inner(temperature: &[u8]) -> i16 {
    let to_digit = |b: u8| -> i16 { (b - b'0') as i16 };
    // Single digit temperature.
    if get_byte(temperature, 1) == b'.' {
        to_digit(get_byte(temperature, 0)) * 10 + to_digit(get_byte(temperature, 2))
    } else {
        to_digit(get_byte(temperature, 0)) * 100
            + to_digit(get_byte(temperature, 1)) * 10
            + to_digit(get_byte(temperature, 3))
    }
}

fn parse_temperature(temperature: &[u8]) -> i16 {
    if get_byte(temperature, 0) == b'-' {
        -parse_temperature_inner(unsafe { temperature.get_unchecked(1..) })
    } else {
        parse_temperature_inner(temperature)
    }
}

#[cfg(target_feature = "avx2")]
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
            buf.get_unchecked(..sep_pos),
            buf.get_unchecked(sep_pos + 1..eol_pos),
            buf.get_unchecked(eol_pos + 1..),
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
    let mut stats: HashMap<Station, Weather> = HashMap::with_capacity(10_000);

    while !buf.is_empty() {
        if get_byte(buf, 0) == b'#' {
            continue;
        }
        let (station, temperature, remainder) = split_line(buf);
        buf = remainder;
        let temperature = parse_temperature(temperature);

        let station = Station(station);
        stats
            .entry(station)
            .and_modify(|entry| {
                if temperature < entry.min {
                    entry.min = temperature;
                } else if temperature > entry.max {
                    entry.max = temperature;
                }

                entry.mean += temperature as i64;
                entry.samples += 1;
            })
            .or_insert(Weather {
                samples: 1,
                min: temperature,
                mean: temperature as i64,
                max: temperature,
            });
    }

    print(stats);
}

fn print(stats: HashMap<Station, Weather>) {
    let stdout = std::io::stdout().lock();
    let mut writer = std::io::BufWriter::new(stdout);
    use std::io::Write;

    let stats: BTreeMap<&str, (f64, f64, f64)> = stats
        .into_iter()
        .map(|(station, weather)| {
            (
                unsafe { str::from_utf8_unchecked(station.0) },
                (
                    weather.min as f64 / 10.0,
                    (weather.mean as f64 / 10.0 / weather.samples as f64),
                    weather.max as f64 / 10.0,
                ),
            )
        })
        .collect();

    write!(writer, "{{").unwrap();
    let mut stats = stats.into_iter().peekable();
    while let Some((station, temperatures)) = stats.next() {
        write!(
            writer,
            "{}={:.1}/{:.1}/{:.1}",
            station, temperatures.0, temperatures.1, temperatures.2,
        )
        .unwrap();

        if stats.peek().is_some() {
            write!(writer, ", ").unwrap();
        }
    }
    writeln!(writer, "}}").unwrap();
}
