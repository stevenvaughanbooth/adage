"""Hidden test for W-2045: Add payment terms."""
import unittest
from acme.generated import models

class TestPaymentTermsField(unittest.TestCase):
    def test_invoice_has_payment_terms_field(self):
        invoice = models.Invoice(
            id="inv-1",
            customer_id="cust-1",
            amount_cents=10000
        )
        # Should have payment_terms attribute with default NET30
        self.assertTrue(hasattr(invoice, 'payment_terms'))
        self.assertEqual(invoice.payment_terms, "NET30")

    def test_invoice_payment_terms_customizable(self):
        invoice = models.Invoice(
            id="inv-1",
            customer_id="cust-1",
            amount_cents=10000,
            payment_terms="NET60"
        )
        self.assertEqual(invoice.payment_terms, "NET60")
