//! Small CLI over the library: convert amounts, look up rates, list coverage.
use chrono::NaiveDate;
use clap::{Parser, Subcommand, ValueEnum};
use hmrc_rates::{RateType, Rates, Updater, YearEnd, YearMonth};
use rust_decimal::Decimal;

#[derive(Parser)]
#[command(name = "hmrc-rates", version, about = "Official HMRC exchange rates")]
struct Cli {
    /// Fetch the latest rates from HMRC before answering (cached on disk).
    #[arg(long, global = true)]
    refresh: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Convert an amount to GBP at the monthly rate for a date.
    Convert {
        /// Decimal amount, e.g. 1250.99.
        amount: String,
        code: String,
        /// Date in YYYY-MM-DD form.
        date: NaiveDate,
    },
    /// Show a rate for a period (YYYY-MM; spot/average need MM = 03 or 12).
    Rate {
        code: String,
        period: YearMonth,
        #[arg(long, value_enum, default_value_t = Series::Monthly)]
        r#type: Series,
    },
    /// List the published periods of a series.
    List {
        #[arg(value_enum)]
        r#type: Series,
    },
    /// List every currency a series has ever quoted.
    Currencies {
        #[arg(value_enum)]
        r#type: Series,
    },
}

#[derive(Copy, Clone, ValueEnum)]
enum Series {
    Monthly,
    Spot,
    Average,
    Weekly,
}

impl From<Series> for RateType {
    fn from(series: Series) -> RateType {
        match series {
            Series::Monthly => RateType::Monthly,
            Series::Spot => RateType::Spot,
            Series::Average => RateType::Average,
            Series::Weekly => RateType::Weekly,
        }
    }
}

fn year_end(month: YearMonth) -> Result<YearEnd, String> {
    YearEnd::from_month(month)
        .ok_or_else(|| format!("'{month}' is not a spot/average period (use MM = 03 or 12)"))
}

fn load(refresh: bool) -> Result<Rates, Box<dyn std::error::Error>> {
    let updater = Updater::new();
    Ok(if refresh {
        updater.refreshed()?
    } else {
        updater.cached()
    })
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let rates = load(cli.refresh)?;
    match cli.command {
        Command::Convert { amount, code, date } => {
            let amount: Decimal = amount
                .parse()
                .map_err(|e| format!("invalid amount '{amount}': {e}"))?;
            let rate = rates.monthly_rate(&code, date)?;
            println!("{} ({})", rate.to_gbp(amount).round_dp(4), rate.period());
        }
        Command::Rate {
            code,
            period: month,
            r#type,
        } => {
            let rate = match r#type {
                Series::Monthly => rates.monthly_rate(&code, month)?,
                Series::Spot => rates.spot(year_end(month)?)?.rate(&code)?,
                Series::Average => rates.average(year_end(month)?)?.rate(&code)?,
                Series::Weekly => {
                    return Err("use `convert` or `list weekly` for the weekly series".into());
                }
            };
            println!(
                "{} {} per £1 ({})",
                rate.units_per_gbp(),
                rate.currency(),
                rate.period()
            );
        }
        Command::List { r#type } => match r#type {
            Series::Monthly => rates.months().for_each(|m| println!("{m}")),
            Series::Spot => rates.spot_periods().for_each(|p| println!("{p}")),
            Series::Average => rates.average_periods().for_each(|p| println!("{p}")),
            Series::Weekly => rates.weeks().for_each(|w| println!("{w}")),
        },
        Command::Currencies { r#type } => {
            rates
                .currencies(r#type.into())
                .for_each(|c| println!("{c}"));
        }
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
