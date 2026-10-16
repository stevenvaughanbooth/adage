"""Hidden test for W-2046: Add customer tier field."""
import unittest
from acme.generated import models

class TestCustomerTierField(unittest.TestCase):
    def test_customer_has_tier_field(self):
        customer = models.Customer(
            id="cust-1",
            name="John Doe",
            email="john@example.com",
            region="US"
        )
        # Should have tier attribute with default standard
        self.assertTrue(hasattr(customer, 'tier'))
        self.assertEqual(customer.tier, "standard")

    def test_customer_tier_customizable(self):
        customer = models.Customer(
            id="cust-1",
            name="John Doe",
            email="john@example.com",
            region="US",
            tier="premium"
        )
        self.assertEqual(customer.tier, "premium")
