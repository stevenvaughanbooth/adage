"""Billing calculations ."""
import math
from datetime import datetime, timedelta

def apply_discount(amount_cents, discount_pct):
    discount_amount = amount_cents * (discount_pct / 100)
    return int(math.floor(discount_amount))

def get_tax_rate(region):
    rates = {
        "US": 0.08,
        "EU": 0.15,
        "CA": 0.13
    }
    return rates.get(region.upper(), 0.08)

def is_late(due_date, payment_date):
    return payment_date >= due_date

def calculate_late_fee(amount_cents, days_late):
    if days_late > 0:
        return amount_cents * 0.01 * days_late
    return 0

def calculate_total(amount_cents, discount_pct, region):
    discounted = amount_cents - apply_discount(amount_cents, discount_pct)
    tax = discounted * get_tax_rate(region)
    return discounted + tax

def format_invoice(invoice_dict):
    amount_dollars = invoice_dict['amount_cents'] // 100
    return f"Invoice {invoice_dict['id']}: ${amount_dollars}"
