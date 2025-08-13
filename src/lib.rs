use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use thiserror::Error;

const XML_DATA: &[&str] = &[include_str!("../data/exrates-monthly-0825.xml")];

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
    #[error("Failed to parse XML data.")]
    XmlParseError(#[from] roxmltree::Error),
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

impl HMRCMonthlyRatesConverter {
    pub fn new() -> Result<Self, ConversionError> {
        let mut rates = BTreeMap::new();
        for xml_data in XML_DATA {
            Self::parse_xml_data(xml_data, &mut rates)?;
        }
        Ok(Self { rates })
    }

    pub fn from_xml(xml_data: &str) -> Result<Self, ConversionError> {
        let mut rates = BTreeMap::new();
        Self::parse_xml_data(xml_data, &mut rates)?;
        Ok(Self { rates })
    }

    fn parse_xml_data(
        xml_data: &str,
        rates: &mut BTreeMap<NaiveDate, BTreeMap<String, Decimal>>,
    ) -> Result<(), ConversionError> {
        let doc = roxmltree::Document::parse(xml_data)?;
        let root_element = doc.root_element();

        let period_str = root_element
            .attribute("Period")
            .ok_or_else(|| ConversionError::DateParseError("Missing Period attribute".to_string()))?;

        let start_date_str = period_str.split(" to ").next().ok_or_else(|| {
            ConversionError::DateParseError(format!("Invalid Period format: {}", period_str))
        })?;

        let month_date = NaiveDate::parse_from_str(start_date_str, "%d/%b/%Y")
            .map_err(|e| ConversionError::DateParseError(e.to_string()))?;

        let mut month_rates = BTreeMap::new();
        for node in root_element
            .children()
            .filter(|n| n.has_tag_name("exchangeRate"))
        {
            let currency_code = node
                .children()
                .find(|n| n.has_tag_name("currencyCode"))
                .and_then(|n| n.text())
                .ok_or_else(|| {
                    ConversionError::CurrencyNotFound(
                        "Missing currency code".to_string(),
                        month_date,
                    )
                })?;
            let rate_str = node
                .children()
                .find(|n| n.has_tag_name("rateNew"))
                .and_then(|n| n.text())
                .ok_or_else(|| ConversionError::RateParseError("Missing rateNew".to_string()))?;
            let rate =
                Decimal::from_str(rate_str).map_err(|e| ConversionError::RateParseError(e.to_string()))?;
            month_rates.insert(currency_code.to_string(), rate);
        }
        rates.insert(month_date, month_rates);
        Ok(())
    }

    pub fn convert(&self, value: &str, date: NaiveDate) -> Result<GBP, ConversionError> {
        let parts: Vec<&str> = value.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(ConversionError::InvalidInputFormat(value.to_string()));
        }
        let amount =
            Decimal::from_str(parts[0]).map_err(|e| ConversionError::ValueParseError(e.to_string()))?;
        let currency = parts[1].to_uppercase();

        let rate = self.lookup_rate(&currency, date)?;
        let result = amount / rate;
        Ok(GBP(result.round_dp(2)))
    }

    fn lookup_rate(&self, currency: &str, date: NaiveDate) -> Result<Decimal, ConversionError> {
        let month_rates = self
            .rates
            .range(..=date)
            .next_back()
            .map(|(_, rates)| rates)
            .ok_or(ConversionError::DateOutOfRange(date))?;

        month_rates
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
        let converter = HMRCMonthlyRatesConverter::new().unwrap();
        assert!(!converter.rates.is_empty());
    }

    #[test]
    fn test_from_xml() {
        let xml_data = fs::read_to_string("data/exrates-monthly-0825.xml").unwrap();
        let converter = HMRCMonthlyRatesConverter::from_xml(&xml_data).unwrap();
        assert!(!converter.rates.is_empty());
    }

    #[test]
    fn test_convert_usd() {
        let converter = HMRCMonthlyRatesConverter::new().unwrap();
        let date = NaiveDate::from_ymd_opt(2025, 8, 15).unwrap();
        let gbp = converter.convert("100.00 USD", date).unwrap();
        assert_eq!(gbp.to_string(), "£73.85");
    }

    #[test]
    fn test_convert_eur() {
        let converter = HMRCMonthlyRatesConverter::new().unwrap();
        let date = NaiveDate::from_ymd_opt(2025, 8, 15).unwrap();
        let gbp = converter.convert("100.00 EUR", date).unwrap();
        assert_eq!(gbp.to_string(), "£86.60");
    }

    #[test]
    fn test_invalid_input() {
        let converter = HMRCMonthlyRatesConverter::new().unwrap();
        let date = NaiveDate::from_ymd_opt(2025, 8, 15).unwrap();
        let result = converter.convert("100.00", date);
        assert!(matches!(result, Err(ConversionError::InvalidInputFormat(_))));
    }

    #[test]
    fn test_currency_not_found() {
        let converter = HMRCMonthlyRatesConverter::new().unwrap();
        let date = NaiveDate::from_ymd_opt(2025, 8, 15).unwrap();
        let result = converter.convert("100.00 XXX", date);
        assert!(matches!(result, Err(ConversionError::CurrencyNotFound(_, _))));
    }

    #[test]
    fn test_date_out_of_range() {
        let converter = HMRCMonthlyRatesConverter::new().unwrap();
        let date = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        let result = converter.convert("100.00 USD", date);
        assert!(matches!(result, Err(ConversionError::DateOutOfRange(_))));
    }
}
