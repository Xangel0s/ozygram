try:
    from .main import main
except ImportError:
    from ozy_brain.main import main

if __name__ == "__main__":
    raise SystemExit(main())
