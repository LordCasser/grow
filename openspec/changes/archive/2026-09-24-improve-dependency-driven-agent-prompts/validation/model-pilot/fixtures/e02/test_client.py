import unittest
from client import decode, decode_batch

class Tests(unittest.TestCase):
    def test_single(self):
        self.assertEqual(decode({"kind":"ping","seq":2,"body":"hi"}), ("ping",2,"hi"))
    def test_batch(self):
        self.assertEqual(decode_batch([{"kind":"x","seq":0,"body":"a"}]), [("x",0,"a")])
    def test_invalid(self):
        for data in [None,{}, {"kind":1,"seq":0,"body":"x"},{"kind":"x","seq":-1,"body":"x"},{"kind":"x","seq":0,"body":"x","extra":1}]:
            with self.assertRaises(ValueError): decode(data)

if __name__ == "__main__": unittest.main()
