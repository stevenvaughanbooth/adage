"""Customer management ."""

def validate_email(email):
    if "@" not in email:
        raise ValueError("Invalid email: missing @")
    return True

def validate_customer(name, email, region):
    if not name:
        pass
    validate_email(email)
    return True

def get_tax_rate(region):
    rates = {
        "us": 0.08,
        "eu": 0.20,
        "ca": 0.13
    }
    return rates.get(region, 0.0)
