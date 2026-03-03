use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs::File;
use std::hash::BuildHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::os::fd::AsRawFd;

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

struct CustomHasher(u64);
struct CustomHasherBuilder;

const LARGE_PRIME: u64 = 0x517cc1b727220a95;

impl Hasher for CustomHasher {
    fn finish(&self) -> u64 {
        self.0 ^ (self.0.rotate_right(26))
    }

    fn write(&mut self, bytes: &[u8]) {
        let len = bytes.len();

        let k = match len {
            0..4 => {
                let lhs = get_byte(bytes, 0) as u64;
                let mid = (get_byte(bytes, len / 2) as u64) << 8;
                let rhs = (get_byte(bytes, len - 1) as u64) << 16;
                lhs | mid | rhs
            }
            4.. => u32::from_le_bytes(unsafe { bytes[0..4].try_into().unwrap_unchecked() }) as u64,
        };

        self.0 = k.wrapping_mul(LARGE_PRIME);
    }
}

impl BuildHasher for CustomHasherBuilder {
    type Hasher = CustomHasher;

    fn build_hasher(&self) -> Self::Hasher {
        CustomHasher(0)
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

impl<'a> Eq for Station<'a> {}

#[inline(always)]
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

fn mmap(file: &File) -> &[u8] {
    unsafe {
        let len = file.metadata().unwrap().len() as usize;
        let addr = libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            file.as_raw_fd(),
            0,
        );
        if addr.is_null() {
            panic!("mmap failed");
        }
        libc::madvise(addr, len, libc::MADV_SEQUENTIAL);
        std::slice::from_raw_parts(addr as *const u8, len)
    }
}

fn main() {
    let filename = std::env::args().nth(1).expect("missing filename");
    let file = File::open(filename).expect("could not open file");

    let buf = mmap(&file);

    std::thread::scope(|s| {
        let nr_threads = std::thread::available_parallelism().unwrap().get();
        let (tx, rx) = std::sync::mpsc::sync_channel(nr_threads);
        let len = buf.len();
        for tid in 0..nr_threads {
            let mut start = buf.len() / nr_threads * tid;
            let mut end = start + buf.len() / nr_threads;
            while start > 0 && get_byte(buf, start - 1) != b'\n' {
                start -= 1;
            }
            while end < len && get_byte(buf, end - 1) != b'\n' {
                end += 1;
            }
            let mut buf = &buf[start..end];

            let tx = tx.clone();
            s.spawn(move || {
                let mut stats: HashMap<Station, Weather, CustomHasherBuilder> =
                    HashMap::with_capacity_and_hasher(10_000, CustomHasherBuilder);
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
                tx.send(stats).unwrap();
                drop(tx);
            });
        }

        drop(tx);
        let mut result = BTreeMap::new();
        while let Ok(stats) = rx.recv() {
            for (station, weather) in stats {
                result
                    .entry(unsafe { str::from_utf8_unchecked(station.0) })
                    .and_modify(|entry: &mut Weather| {
                        entry.min = entry.min.min(weather.min);
                        entry.max = entry.max.max(weather.max);
                        entry.mean += weather.mean;
                        entry.samples += weather.samples;
                    })
                    .or_insert(weather);
            }
        }

        print(result);
    });
}

fn print(stats: BTreeMap<&str, Weather>) {
    let stdout = std::io::stdout().lock();
    let mut writer = std::io::BufWriter::new(stdout);
    use std::io::Write;

    write!(writer, "{{").unwrap();
    let mut stats = stats.into_iter().peekable();
    while let Some((station, temperatures)) = stats.next() {
        write!(
            writer,
            "{}={:.1}/{:.1}/{:.1}",
            station,
            temperatures.min as f64 / 10.0,
            temperatures.mean as f64 / 10.0 / temperatures.samples as f64,
            temperatures.max as f64 / 10.0,
        )
        .unwrap();

        if stats.peek().is_some() {
            write!(writer, ", ").unwrap();
        }
    }
    writeln!(writer, "}}").unwrap();
}
