"""Hidden test for W-1045: Add currency field to invoices."""
import unittest
from acme.generated import models

class TestCurrencyField(unittest.TestCase):
    def test_invoice_has_currency_field(self):
        invoice = models.Invoice(
            id="inv-1",
            customer_id="cust-1",
            amount_cents=10000
        )
        # Should have currency attribute with default USD
        self.assertTrue(hasattr(invoice, 'currency'))
        self.assertEqual(invoice.currency, "USD")

    def test_invoice_currency_customizable(self):
        invoice = models.Invoice(
            id="inv-1",
            customer_id="cust-1",
            amount_cents=10000,
            currency="EUR"
        )
        self.assertEqual(invoice.currency, "EUR")
