import tests
import unittest
from acme import customers

class TestCustomers(unittest.TestCase):
    def test_email_validation(self):
        customers.validate_email("user@example.com")
        with self.assertRaises(ValueError):
            customers.validate_email("notanemail")
