"""Hidden test for W-1044: Email validation too lenient."""
import unittest
from acme import customers

class TestEmailValidation(unittest.TestCase):
    def test_email_requires_domain_dot(self):
        # Valid emails should pass
        customers.validate_email("user@example.com")
        customers.validate_email("test@domain.co.uk")

        # Invalid emails should fail
        with self.assertRaises(ValueError):
            customers.validate_email("user@localhost")
        with self.assertRaises(ValueError):
            customers.validate_email("user@domain")
