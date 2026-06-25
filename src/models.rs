//! Data structures parsed from the TSETMC server, mirroring the JS classes.

use crate::error::{Error, Result};
use crate::util::clean_fa;

/// Column identifiers, indexed exactly like the JS `cols` array.
pub const COLS: [&str; 15] = [
    "date",
    "dateshamsi",
    "open",
    "high",
    "low",
    "last",
    "close",
    "vol",
    "count",
    "value",
    "yesterday",
    "symbol",
    "name",
    "namelatin",
    "companycode",
];

/// Persian column names, indexed exactly like the JS `colsFa` array.
pub const COLS_FA: [&str; 15] = [
    "تاریخ میلادی",
    "تاریخ شمسی",
    "اولین قیمت",
    "بیشترین قیمت",
    "کمترین قیمت",
    "آخرین قیمت",
    "قیمت پایانی",
    "حجم معاملات",
    "تعداد معاملات",
    "ارزش معاملات",
    "قیمت پایانی دیروز",
    "نماد",
    "نام",
    "نام لاتین",
    "کد شرکت",
];

/// A single closing-price row. All numeric fields are kept as strings to
/// preserve the exact textual form (the JS does the same and only converts
/// when needed), which matters for decimal-accurate price adjustment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosingPrice {
    pub ins_code: String,        // InsCode (int64)
    pub deven: String,           // DEven (int32)
    pub pclosing: String,        // close
    pub pdr_cot_val: String,     // last
    pub ztot_tran: String,       // count
    pub qtot_tran5j: String,     // volume
    pub qtot_cap: String,        // value
    pub price_min: String,       // low
    pub price_max: String,       // high
    pub price_yesterday: String, // yesterday
    pub price_first: String,     // open
}

impl ClosingPrice {
    /// Parse from a comma-separated row (11 fields).
    pub fn parse(row: &str) -> Result<Self> {
        let f: Vec<&str> = row.split(',').collect();
        if f.len() != 11 {
            return Err(Error::InvalidData("Invalid ClosingPrice data!".into()));
        }
        Ok(ClosingPrice {
            ins_code: f[0].to_string(),
            deven: f[1].to_string(),
            pclosing: f[2].to_string(),
            pdr_cot_val: f[3].to_string(),
            ztot_tran: f[4].to_string(),
            qtot_tran5j: f[5].to_string(),
            qtot_cap: f[6].to_string(),
            price_min: f[7].to_string(),
            price_max: f[8].to_string(),
            price_yesterday: f[9].to_string(),
            price_first: f[10].to_string(),
        })
    }

    /// Serialize back to a comma-separated row, preserving field order.
    pub fn to_csv_row(&self) -> String {
        [
            &self.ins_code,
            &self.deven,
            &self.pclosing,
            &self.pdr_cot_val,
            &self.ztot_tran,
            &self.qtot_tran5j,
            &self.qtot_cap,
            &self.price_min,
            &self.price_max,
            &self.price_yesterday,
            &self.price_first,
        ]
        .map(|s| s.as_str())
        .join(",")
    }
}

/// A resolved output column (name + Persian name + header).
#[derive(Debug, Clone)]
pub struct Column {
    pub name: &'static str,
    pub fname: &'static str,
    pub header: String,
}

impl Column {
    /// Build from a column index and optional header override.
    pub fn new(index: usize, header: Option<String>) -> Result<Self> {
        if index >= COLS.len() {
            return Err(Error::InvalidData("Invalid Column data!".into()));
        }
        let name = COLS[index];
        let header = header
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| name.to_string());
        Ok(Column {
            name,
            fname: COLS_FA[index],
            header,
        })
    }
}

/// An instrument (symbol) descriptor.
#[derive(Debug, Clone)]
pub struct Instrument {
    pub ins_code: String,
    pub instrument_id: String,
    pub latin_symbol: String,
    pub latin_name: String,
    pub company_code: String,
    pub symbol: String,
    pub name: String,
    pub cisin: String,
    pub deven: String,
    pub flow: String,
    pub lsoc30: String,
    pub cgds_val: String,
    pub cgr_val_cot: String,
    pub ymar_nsc: String,
    pub ccom_val: String,
    pub csec_val: String,
    pub cso_sec_val: String,
    pub yval: String,
    pub symbol_original: Option<String>,
}

impl Instrument {
    pub fn parse(row: &str) -> Result<Self> {
        let f: Vec<&str> = row.split(',').collect();
        if f.len() != 18 && f.len() != 19 {
            return Err(Error::InvalidData("Invalid Instrument data!".into()));
        }
        Ok(Instrument {
            ins_code: f[0].to_string(),
            instrument_id: f[1].to_string(),
            latin_symbol: f[2].to_string(),
            latin_name: f[3].to_string(),
            company_code: f[4].to_string(),
            symbol: clean_fa(f[5]).trim().to_string(),
            name: f[6].to_string(),
            cisin: f[7].to_string(),
            deven: f[8].to_string(),
            flow: f[9].to_string(),
            lsoc30: f[10].to_string(),
            cgds_val: f[11].to_string(),
            cgr_val_cot: f[12].to_string(),
            ymar_nsc: f[13].to_string(),
            ccom_val: f[14].to_string(),
            csec_val: f[15].to_string(),
            cso_sec_val: f[16].to_string(),
            yval: f[17].to_string(),
            symbol_original: f
                .get(18)
                .filter(|s| !s.is_empty())
                .map(|s| clean_fa(s).trim().to_string()),
        })
    }
}

/// Intraday instrument descriptor.
#[derive(Debug, Clone)]
pub struct InstrumentItd {
    pub ins_code: String,
    pub lval30: String,
    pub lval18afc: String,
    pub flow_title: String,
    pub cgr_val_cot_title: String,
    pub flow: String,
    pub cgr_val_cot: String,
    pub cisin: String,
    pub instrument_id: String,
    pub ztitad: String,
    pub base_vol: String,
}

impl InstrumentItd {
    pub fn parse(row: &str) -> Result<Self> {
        let f: Vec<&str> = row.split(',').collect();
        if f.len() != 11 {
            return Err(Error::InvalidData("Invalid InstrumentITD data!".into()));
        }
        Ok(InstrumentItd {
            ins_code: f[0].to_string(),
            lval30: clean_fa(f[1]),
            lval18afc: clean_fa(f[2]),
            flow_title: clean_fa(f[3]),
            cgr_val_cot_title: clean_fa(f[4]),
            flow: f[5].to_string(),
            cgr_val_cot: f[6].to_string(),
            cisin: f[7].to_string(),
            instrument_id: f[8].to_string(),
            ztitad: f[9].to_string(),
            base_vol: f[10].to_string(),
        })
    }
}

/// A share-count change record (used for capital-increase adjustments).
#[derive(Debug, Clone)]
pub struct Share {
    pub idn: String,
    pub ins_code: String,
    pub deven: String,
    pub number_of_share_new: i64,
    pub number_of_share_old: i64,
}

impl Share {
    pub fn parse(row: &str) -> Result<Self> {
        let f: Vec<&str> = row.split(',').collect();
        if f.len() != 5 {
            return Err(Error::InvalidData("Invalid Share data!".into()));
        }
        Ok(Share {
            idn: f[0].to_string(),
            ins_code: f[1].to_string(),
            deven: f[2].to_string(),
            number_of_share_new: f[3].trim().parse().unwrap_or(0),
            number_of_share_old: f[4].trim().parse().unwrap_or(0),
        })
    }
}

/// Intraday group column definitions, mirroring `itdGroupCols`.
pub fn itd_group_cols() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            "price",
            vec![
                "time",
                "last",
                "close",
                "open",
                "high",
                "low",
                "count",
                "volume",
                "value",
                "discarded",
            ],
        ),
        (
            "order",
            vec![
                "time", "row", "askcount", "askvol", "askprice", "bidprice", "bidvol", "bidcount",
            ],
        ),
        (
            "trade",
            vec!["time", "count", "volume", "price", "discarded"],
        ),
        (
            "client",
            vec![
                "pbvol", "pbcount", "pbval", "pbprice", "pbvolpot", "psvol", "pscount", "psval",
                "psprice", "psvolpot", "lbvol", "lbcount", "lbval", "lbprice", "lbvolpot", "lsvol",
                "lscount", "lsval", "lsprice", "lsvolpot", "lpchg",
            ],
        ),
        ("misc", vec!["basevol", "flow", "daymin", "daymax", "state"]),
        (
            "shareholder",
            vec![
                "shares",
                "sharespot",
                "change",
                "companycode",
                "companyname",
            ],
        ),
    ]
}
