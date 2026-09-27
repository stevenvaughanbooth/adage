from dataclasses import dataclass, field

@dataclass
class Customer:
    id: str
    name: str = ''
    email: str = ''
    region: str = 'US'

@dataclass
class Invoice:
    id: str
    customer_id: str
    amount_cents: int = 0
    tax_region: str = 'US'
    discount_pct: float = 0.0
