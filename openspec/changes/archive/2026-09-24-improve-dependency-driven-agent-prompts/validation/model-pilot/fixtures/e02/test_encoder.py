import unittest
from encoder import encode, encode_batch

class Tests(unittest.TestCase):
    def test_single(self):
        self.assertEqual(encode("ping", 2, "hi"), {"kind":"ping","seq":2,"body":"hi"})
    def test_batch(self):
        self.assertEqual(encode_batch([("x",0,"a"),("y",1,"b")]), [{"kind":"x","seq":0,"body":"a"},{"kind":"y","seq":1,"body":"b"}])
    def test_invalid(self):
        for data in [(1,0,"x"),("x",-1,"x"),("x",0,2)]:
            with self.assertRaises(ValueError): encode(*data)

if __name__ == "__main__": unittest.main()
