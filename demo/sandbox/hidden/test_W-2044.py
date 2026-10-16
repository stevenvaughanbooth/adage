"""Hidden test for W-2044: Empty name validation."""
import unittest
from acme import customers

class TestNameValidation(unittest.TestCase):
    def test_empty_name_validation(self):
        # Empty name should raise error
        with self.assertRaises(ValueError):
            customers.validate_customer("", "user@example.com", "US")

        # Valid name should pass
        customers.validate_customer("John Doe", "john@example.com", "US")
