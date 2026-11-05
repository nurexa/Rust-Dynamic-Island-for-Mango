//! Calculator and unit conversion for the search box: "2^10/3", "sqrt(2)*pi",
//! "10 km to mi", "72 f in c", "3.5 GiB to MB". Nothing is listed until the query
//! is one of these, so it costs nothing while you search for apps.

/// What a query works out to, ready to show and copy. None when it isn't math.
pub fn answer(query: &str) -> Option<(String, String)> {
    let q = query.trim();
    if let Some(r) = convert(q) {
        return Some(r);
    }
    // A bare number or word isn't a calculation: "2" and "pi" are searches.
    let mathy = q.chars().any(|c| "+-*/^%()".contains(c)) || FUNCS.iter().any(|(f, _)| q.contains(&format!("{f}(")));
    if !mathy || !q.chars().any(|c| c.is_ascii_digit() || c == '(') {
        return None;
    }
    let v = eval(q)?;
    let shown = fmt(v);
    Some((format!("= {shown}"), shown))
}

const FUNCS: &[(&str, fn(f64) -> f64)] = &[
    ("sqrt", f64::sqrt), ("abs", f64::abs), ("ln", f64::ln), ("log", f64::log10),
    ("sin", f64::sin), ("cos", f64::cos), ("tan", f64::tan), ("round", f64::round),
    ("floor", f64::floor), ("ceil", f64::ceil),
];

/// Recursive descent over + - * / % ^, parentheses, unary minus, pi, e and FUNCS.
pub fn eval(s: &str) -> Option<f64> {
    let mut p = Parser { s: s.as_bytes(), i: 0 };
    let v = p.sum()?;
    p.ws();
    (p.i == p.s.len() && v.is_finite()).then_some(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        let hit = self.s.get(self.i) == Some(&c);
        self.i += hit as usize;
        hit
    }

    fn sum(&mut self) -> Option<f64> {
        let mut v = self.product()?;
        loop {
            if self.eat(b'+') {
                v += self.product()?;
            } else if self.eat(b'-') {
                v -= self.product()?;
            } else {
                return Some(v);
            }
        }
    }

    fn product(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        loop {
            if self.eat(b'*') || self.eat(b'x') {
                v *= self.power()?;
            } else if self.eat(b'/') {
                v /= self.power()?;
            } else if self.eat(b'%') {
                v %= self.power()?;
            } else {
                return Some(v);
            }
        }
    }

    /// Right-associative, and above unary minus: -2^2 is -4.
    fn power(&mut self) -> Option<f64> {
        if self.eat(b'-') {
            return Some(-self.power()?);
        }
        let base = self.atom()?;
        if self.eat(b'^') { Some(base.powf(self.power()?)) } else { Some(base) }
    }

    fn atom(&mut self) -> Option<f64> {
        self.ws();
        if self.eat(b'(') {
            let v = self.sum()?;
            return self.eat(b')').then_some(v);
        }
        let start = self.i;
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || *c == b'.' || *c == b'_' || *c == b',') {
            self.i += 1;
        }
        if self.i > start {
            let n: String = self.s[start..self.i].iter().filter(|c| **c != b'_' && **c != b',').map(|&c| c as char).collect();
            return n.parse().ok();
        }
        while self.s.get(self.i).is_some_and(u8::is_ascii_alphabetic) {
            self.i += 1;
        }
        let word = std::str::from_utf8(&self.s[start..self.i]).ok()?;
        match word {
            "pi" => Some(std::f64::consts::PI),
            "e" => Some(std::f64::consts::E),
            _ => {
                let f = FUNCS.iter().find(|(n, _)| *n == word)?.1;
                Some(f(self.atom()?))
            }
        }
    }
}

/// Up to 10 significant decimals, no trailing zeros, scientific when huge or tiny.
pub fn fmt(v: f64) -> String {
    if v != 0.0 && !(1e-6..1e15).contains(&v.abs()) {
        return format!("{v:e}");
    }
    let s = format!("{v:.10}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

// Each unit: names it answers to, and its size in the dimension's base unit.
type Units = &'static [(&'static [&'static str], f64)];
const DIMENSIONS: &[Units] = &[
    &[(&["m", "meter", "meters", "metre", "metres"], 1.0), (&["km", "kilometer", "kilometers", "kilometre", "kilometres"], 1e3),
      (&["cm", "centimeter", "centimeters"], 0.01), (&["mm", "millimeter", "millimeters"], 1e-3), (&["um", "micrometer"], 1e-6),
      (&["nm", "nanometer", "nanometers"], 1e-9), (&["mi", "mile", "miles"], 1609.344), (&["yd", "yard", "yards"], 0.9144),
      (&["ft", "foot", "feet"], 0.3048), (&["in", "inch", "inches"], 0.0254), (&["nmi"], 1852.0)],
    &[(&["kg", "kilogram", "kilograms", "kilo", "kilos"], 1.0), (&["g", "gram", "grams"], 1e-3), (&["mg", "milligram", "milligrams"], 1e-6),
      (&["t", "tonne", "tonnes", "ton", "tons"], 1e3), (&["lb", "lbs", "pound", "pounds"], 0.45359237),
      (&["oz", "ounce", "ounces"], 0.028349523125), (&["st", "stone"], 6.35029318)],
    &[(&["s", "sec", "secs", "second", "seconds"], 1.0), (&["ms", "millisecond", "milliseconds"], 1e-3),
      (&["min", "mins", "minute", "minutes"], 60.0), (&["h", "hr", "hrs", "hour", "hours"], 3600.0),
      (&["d", "day", "days"], 86400.0), (&["wk", "week", "weeks"], 604800.0), (&["yr", "year", "years"], 31_557_600.0)],
    &[(&["b", "byte", "bytes"], 1.0), (&["kb"], 1e3), (&["mb"], 1e6), (&["gb"], 1e9), (&["tb"], 1e12),
      (&["kib"], 1024.0), (&["mib"], 1_048_576.0), (&["gib"], 1_073_741_824.0), (&["tib"], 1_099_511_627_776.0), (&["bit", "bits"], 0.125)],
    &[(&["l", "liter", "liters", "litre", "litres"], 1.0), (&["ml", "milliliter", "milliliters"], 1e-3),
      (&["gal", "gallon", "gallons"], 3.785411784), (&["qt", "quart", "quarts"], 0.946352946), (&["pt", "pint", "pints"], 0.473176473),
      (&["cup", "cups"], 0.2365882365), (&["floz"], 0.0295735295625), (&["tbsp"], 0.01478676478125), (&["tsp"], 0.00492892159375)],
    &[(&["m/s"], 1.0), (&["km/h", "kmh", "kph"], 1.0 / 3.6), (&["mph"], 0.44704), (&["kn", "knot", "knots"], 0.514444)],
    &[(&["m2", "sqm"], 1.0), (&["km2"], 1e6), (&["ha", "hectare", "hectares"], 1e4), (&["acre", "acres"], 4046.8564224),
      (&["ft2", "sqft"], 0.09290304), (&["mi2"], 2_589_988.110336)],
    &[(&["j", "joule", "joules"], 1.0), (&["kj"], 1e3), (&["cal"], 4.184), (&["kcal"], 4184.0), (&["wh"], 3600.0), (&["kwh"], 3.6e6)],
];

fn unit(name: &str) -> Option<(usize, f64)> {
    let n = name.to_lowercase();
    DIMENSIONS.iter().enumerate().find_map(|(d, units)| units.iter().find(|(names, _)| names.contains(&n.as_str())).map(|u| (d, u.1)))
}

fn temperature(name: &str) -> Option<char> {
    match name.to_lowercase().trim_start_matches('°') {
        "c" | "celsius" => Some('C'),
        "f" | "fahrenheit" => Some('F'),
        "k" | "kelvin" => Some('K'),
        _ => None,
    }
}

/// "<amount> <unit> to|in|as <unit>". The amount can itself be a calculation.
fn convert(q: &str) -> Option<(String, String)> {
    let lower = q.to_lowercase();
    let (left, to) = [" to ", " in ", " as ", "->"].iter().find_map(|sep| {
        let at = lower.rfind(sep)?;
        Some((&q[..at], q[at + sep.len()..].trim()))
    })?;
    let left = left.trim();
    // The unit is the trailing run of letters (and / or 2), so "10km" and "10 km" both work.
    let split = left.rfind(|c: char| !(c.is_alphabetic() || c == '/' || c == '2' || c == '°')).map_or(0, |i| i + 1);
    let (amount, from) = (left[..split].trim(), &left[split..]);
    let v = if amount.is_empty() { 1.0 } else { eval(amount)? };
    let out = match (temperature(from), temperature(to)) {
        (Some(a), Some(b)) => {
            let c = match a { 'C' => v, 'F' => (v - 32.0) * 5.0 / 9.0, _ => v - 273.15 };
            match b { 'C' => c, 'F' => c * 9.0 / 5.0 + 32.0, _ => c + 273.15 }
        }
        _ => {
            let ((da, a), (db, b)) = (unit(from)?, unit(to)?);
            if da != db {
                return None;
            }
            v * a / b
        }
    };
    let shown = fmt(out);
    Some((format!("{shown} {to}"), shown))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_follows_the_usual_rules() {
        assert_eq!(eval("2+3*4"), Some(14.0));
        assert_eq!(eval("(2+3)*4"), Some(20.0));
        assert_eq!(eval("2^3^2"), Some(512.0));
        assert_eq!(eval("-2^2"), Some(-4.0));
        assert_eq!(eval("10 % 4"), Some(2.0));
        assert_eq!(eval("1,000 * 2"), Some(2000.0));
        assert_eq!(fmt(eval("sqrt(2)").unwrap()), "1.4142135624");
        assert_eq!(eval("1/0"), None, "infinity isn't an answer");
        assert_eq!(eval("2 +"), None);
    }

    #[test]
    fn searches_are_left_alone() {
        assert_eq!(answer("firefox"), None);
        assert_eq!(answer("2"), None);
        assert_eq!(answer("pi"), None);
        assert_eq!(answer("code-oss"), None);
        assert_eq!(answer("2*pi").map(|a| a.1), Some("6.2831853072".into()));
    }

    #[test]
    fn converts_units_and_temperatures() {
        assert_eq!(answer("10 km to mi").map(|a| a.1), Some("6.2137119224".into()));
        assert_eq!(answer("10km in m").map(|a| a.0), Some("10000 m".into()));
        assert_eq!(answer("212 F to C").map(|a| a.1), Some("100".into()));
        assert_eq!(answer("1 GiB to MB").map(|a| a.1), Some("1073.741824".into()));
        assert_eq!(answer("2*3 hours to min").map(|a| a.1), Some("360".into()));
        assert_eq!(answer("5 kg to km"), None, "different dimensions don't convert");
        assert_eq!(answer("songs in spotify"), None);
    }
}
