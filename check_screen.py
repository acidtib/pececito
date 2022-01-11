import time
from PIL import ImageGrab

for window in range(10):
  time.sleep(2)

  base_screen = ImageGrab.grab(bbox=(0, 0, 1920, 1045))
  base_screen.save('catch/base_screen.png')  # Save to file
