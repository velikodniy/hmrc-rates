use std::collections::BTreeMap;
use std::fmt;

use std::str::FromStr;

use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use thiserror::Error;
use xml::reader::{EventReader, XmlEvent};

use include_dir::{include_dir, Dir};

const XML_FILES: Dir = include_dir!("data");

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GBP(Decimal);

impl GBP {
    pub fn as_decimal(&self) -> &Decimal {
        &self.0
    }
}

impl fmt::Display for GBP {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "£{}", self.0)
    }
}

#[derive(Debug, Error)]
pub enum ConversionError {
    #[error("Invalid input format: '{0}'. Expected format 'VALUE CURRENCY'.")]
    InvalidInputFormat(String),
    #[error("Currency not found: '{0}' for date {1}.")]
    CurrencyNotFound(String, NaiveDate),
    #[error("No exchange rate data available for date: {0}.")]
    DateOutOfRange(NaiveDate),
    #[error("Failed to parse XML data: {0}")]
    XmlParseError(#[from] xml::reader::Error),
    #[error("Failed to parse date: {0}")]
    DateParseError(String),
    #[error("Failed to parse rate: {0}")]
    RateParseError(String),
    #[error("Failed to parse value: {0}")]
    ValueParseError(String),
}

pub struct HMRCMonthlyRatesConverter {
    rates: BTreeMap<NaiveDate, BTreeMap<String, Decimal>>,
}

impl Default for HMRCMonthlyRatesConverter {
    fn default() -> Self {
        Self { rates: BTreeMap::new() }
    }
}

impl HMRCMonthlyRatesConverter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_default_rates() -> Result<Self, ConversionError> {
        let mut converter = Self::new();
        converter.load_default_rates()?;
        Ok(converter)
    }

    fn load_default_rates(&mut self) -> Result<(), ConversionError> {
        for file in XML_FILES.files() {
            let xml_data = file.contents();
            Self::parse_xml_data(xml_data, &mut self.rates)?;
        }
        Ok(())
    }

    pub fn from_xml(xml_data: &[u8]) -> Result<Self, ConversionError> {
        let mut rates = BTreeMap::new();
        Self::parse_xml_data(xml_data, &mut rates)?;
        Ok(Self { rates })
    }



    fn parse_xml_data(
        xml_data: &[u8],
        rates: &mut BTreeMap<NaiveDate, BTreeMap<String, Decimal>>,
    ) -> Result<(), ConversionError> {
        let parser = EventReader::new(xml_data);
        let mut month_date: Option<NaiveDate> = None;
        let mut month_rates = BTreeMap::new();
        let mut in_currency_code = false;
        let mut in_rate = false;
        let mut currency_code = String::new();

        for e in parser {
            match e? {
                XmlEvent::StartElement { name, attributes, .. } => {
                    if name.local_name == "exchangeRateMonthList" {
                        for attr in attributes {
                            if attr.name.local_name == "Period" {
                                let mut parts = attr.value.split(" to ");
                                let start_date_str = parts.next().ok_or_else(||
                                    ConversionError::DateParseError(format!(
                                        "Invalid Period format: {}",
                                        attr.value
                                    ))
                                )?;
                                let end_date_str = parts.next().ok_or_else(||
                                    ConversionError::DateParseError(format!(
                                        "Invalid Period format: {}",
                                        attr.value
                                    ))
                                )?;

                                let start_date = NaiveDate::parse_from_str(start_date_str, "%d/%b/%Y")
                                    .map_err(|e| ConversionError::DateParseError(e.to_string()))?;

                                if start_date.day() != 1 {
                                    return Err(ConversionError::DateParseError(
                                        "Period start date is not the 1st of the month".to_string(),
                                    ));
                                }

                                let end_date = NaiveDate::parse_from_str(end_date_str, "%d/%b/%Y")
                                    .map_err(|e| ConversionError::DateParseError(e.to_string()))?;

                                let last_day_of_month = NaiveDate::from_ymd_opt(
                                    start_date.year(),
                                    start_date.month() + 1,
                                    1,
                                )
                                .unwrap_or(NaiveDate::from_ymd_opt(start_date.year() + 1, 1, 1).unwrap())
                                .pred_opt()
                                .unwrap();

                                if end_date != last_day_of_month {
                                    return Err(ConversionError::DateParseError(
                                        "Period end date is not the last day of the month".to_string(),
                                    ));
                                }

                                month_date = Some(start_date);
                            }
                        }
                    } else if name.local_name == "currencyCode" {
                        in_currency_code = true;
                    } else if name.local_name == "rateNew" {
                        in_rate = true;
                    }
                }
                XmlEvent::Characters(s) => {
                    if in_currency_code {
                        currency_code = s;
                    } else if in_rate {
                        let rate = Decimal::from_str(&s)
                            .map_err(|e| ConversionError::RateParseError(e.to_string()))?;
                        month_rates.insert(currency_code.clone(), rate);
                    }
                }
                XmlEvent::EndElement { name } => {
                    if name.local_name == "currencyCode" {
                        in_currency_code = false;
                    } else if name.local_name == "rateNew" {
                        in_rate = false;
                    }
                }
                _ => {}
            }
        }

        if let Some(date) = month_date {
            rates.insert(date, month_rates);
        }

        Ok(())
    }

    pub fn convert(&self, amount: Decimal, currency: &str, date: NaiveDate) -> Result<GBP, ConversionError> {
        let currency = currency.to_uppercase();
        let rate = self.lookup_rate(&currency, date)?;
        let result = amount / rate;
        Ok(GBP(result.round_dp(2)))
    }

    fn lookup_rate(&self, currency: &str, date: NaiveDate) -> Result<Decimal, ConversionError> {
        self.rates
            .range(..=date)
            .next_back()
            .map(|(_, rates)| rates)
            .ok_or(ConversionError::DateOutOfRange(date))?
            .get(currency)
            .cloned()
            .ok_or_else(|| ConversionError::CurrencyNotFound(currency.to_string(), date))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_new_converter() {
        let converter = HMRCMonthlyRatesConverter::with_default_rates().unwrap();
        assert!(!converter.rates.is_empty());
    }

    #[test]
    fn test_from_xml() {
        let xml_data = fs::read("data/exrates-monthly-0825.xml").unwrap();
        let converter = HMRCMonthlyRatesConverter::from_xml(&xml_data).unwrap();
        assert!(!converter.rates.is_empty());
    }

    use rust_decimal_macros::dec;

    #[test]
    fn test_convert_usd() {
        let converter = HMRCMonthlyRatesConverter::with_default_rates().unwrap();
        let date = NaiveDate::from_ymd_opt(2025, 8, 15).unwrap();
        let gbp = converter.convert(dec!(100.00), "USD", date).unwrap();
        assert_eq!(gbp.to_string(), "£73.85");
    }

    #[test]
    fn test_convert_eur() {
        let converter = HMRCMonthlyRatesConverter::with_default_rates().unwrap();
        let date = NaiveDate::from_ymd_opt(2025, 8, 15).unwrap();
        let gbp = converter.convert(dec!(100.00), "EUR", date).unwrap();
        assert_eq!(gbp.to_string(), "£86.60");
    }

    #[test]
    fn test_invalid_input() {
        let converter = HMRCMonthlyRatesConverter::with_default_rates().unwrap();
        let date = NaiveDate::from_ymd_opt(2025, 8, 15).unwrap();
        let result = converter.convert(dec!(100.00), "", date);
        assert!(matches!(result, Err(ConversionError::CurrencyNotFound(_, _))));
    }

    #[test]
    fn test_currency_not_found() {
        let converter = HMRCMonthlyRatesConverter::with_default_rates().unwrap();
        let date = NaiveDate::from_ymd_opt(2025, 8, 15).unwrap();
        let result = converter.convert(dec!(100.00), "XXX", date);
        assert!(matches!(result, Err(ConversionError::CurrencyNotFound(_, _))));
    }

    #[test]
    fn test_date_out_of_range() {
        let converter = HMRCMonthlyRatesConverter::with_default_rates().unwrap();
        let date = NaiveDate::from_ymd_opt(2014, 12, 31).unwrap();
        let result = converter.convert(dec!(100.00), "USD", date);
        assert!(matches!(result, Err(ConversionError::DateOutOfRange(_))));
    }

    #[test]
    fn test_convert_on_first_day_of_month() {
        let converter = HMRCMonthlyRatesConverter::with_default_rates().unwrap();
        let date = NaiveDate::from_ymd_opt(2025, 8, 1).unwrap();
        let gbp = converter.convert(dec!(100.00), "USD", date).unwrap();
        assert_eq!(gbp.to_string(), "£73.85");
    }

    #[test]
    fn test_convert_on_last_day_of_month() {
        let converter = HMRCMonthlyRatesConverter::with_default_rates().unwrap();
        let date = NaiveDate::from_ymd_opt(2025, 8, 31).unwrap();
        let gbp = converter.convert(dec!(100.00), "USD", date).unwrap();
        assert_eq!(gbp.to_string(), "£73.85");
    }

    #[test]
    fn test_malformed_period() {
        let xml_data = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<exchangeRateMonthList Period=\"02/Aug/2025 to 31/Aug/2025\">\n</exchangeRateMonthList>\n";
        let result = HMRCMonthlyRatesConverter::from_xml(xml_data.as_bytes());
        assert!(matches!(result, Err(ConversionError::DateParseError(_))));
    }

    #[test]
    fn test_incorrect_end_date() {
        let xml_data = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<exchangeRateMonthList Period=\"01/Aug/2025 to 30/Aug/2025\">\n</exchangeRateMonthList>\n";
        let result = HMRCMonthlyRatesConverter::from_xml(xml_data.as_bytes());
        assert!(matches!(result, Err(ConversionError::DateParseError(_))));
    }
}
