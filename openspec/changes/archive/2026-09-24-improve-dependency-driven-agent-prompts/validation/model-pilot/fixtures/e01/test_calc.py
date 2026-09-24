import unittest
from calc import clamp

class ClampTests(unittest.TestCase):
    def test_clamp(self):
        self.assertEqual(clamp(-2, 0, 10), 0)
        self.assertEqual(clamp(5, 0, 10), 5)
        self.assertEqual(clamp(12, 0, 10), 10)

if __name__ == "__main__":
    unittest.main()
