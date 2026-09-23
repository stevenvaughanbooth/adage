"""Hidden test for W-2041: Fix invoice formatting precision."""
import unittest
from acme import billing

class TestInvoiceFormatting(unittest.TestCase):
    def test_invoice_format_preserves_cents(self):
        # Invoice with cents should include them in formatted output
        invoice = {
            'id': 'INV-001',
            'amount_cents': 15050  # $150.50
        }
        formatted = billing.format_invoice(invoice)
        # Should include cents, not just dollars
        self.assertIn("150.50", formatted, f"Expected cents in output, got: {formatted}")
