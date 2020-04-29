import time
import cv2
import numpy as np
import pyautogui
from pywinauto import Application
from PIL import ImageGrab
from random import randint

x = None
y = None

windowTitle = 'World of Warcraft'
matching_threshold = 0.6
white_pixel_threshold = 1

template = cv2.imread('template/night.png', 0)
w, h = template.shape[::-1]

print('Starting bot')
time.sleep(5)

# bring game window forward
wow = Application().connect(path="WowClassic.exe", title=windowTitle)
wow.WorldofWarcraft.set_focus()

def ran():
    return randint(0, 600)

def whitepx():
    current = cv2.cvtColor(np.array(cv2.imread('template/bob.png')), cv2.COLOR_BGRA2GRAY)
    H_B, W_B = current.shape
    white_pixels = 0
    i = 0
    while i < H_B:
        j = 0
        print('Debugging. x: {}, y: {}, pixel: {}'.format(x, y, white_pixels))
        while j < W_B:
            if current[i][j] > 245:
                white_pixels = white_pixels + 1
            if white_pixels > white_pixel_threshold:
                return True
            j = j + 1
        i = i + 1
    return False

def cast_it():
    print('Casting!') 
    pyautogui.typewrite("=")  # Cast fishing skill on =
    time.sleep(2)

def click_it():
    pyautogui.mouseDown(button='right')
    time.sleep(0.4)
    
    pyautogui.mouseUp(button='right')
    time.sleep(0.5)


for fishing in range(5):
    pyautogui.moveTo(ran(), ran())
    x = None
    y = None

    cast_it()

    for search_bob in range(5):
        if x is None:
            time.sleep(1)
            # TODO: improve base screenshot, only capture game window
            base_screen = ImageGrab.grab(bbox=(0, 0, 1450, 800))
            base_screen.save('base_screen.png')  # Save to file

            img_rgb = cv2.imread('base_screen.png') # Read base
            img_gray = cv2.cvtColor(img_rgb, cv2.COLOR_BGR2GRAY)

            res = cv2.matchTemplate(img_gray, template, cv2.TM_CCOEFF_NORMED) # Search for match
            loc = np.where(res >= matching_threshold)

            for pt in zip(*loc[::-1]):
                x = int(pt[0])
                y = int(pt[1])
                cv2.rectangle(img_rgb, pt, (pt[0] + w, pt[1] + h), (0,255,255), 1)
                cv2.imwrite('base_matched.png', img_rgb)
        else:
            # move mouse to bob
            pyautogui.moveTo(x + w / 2, y + h / 2, 1, pyautogui.easeOutQuad)

            for bite in range(120):
                clean_screen = ImageGrab.grab(bbox=(x, y, x + w, y + h))
                clean_screen.save('template/bob.png')
                time.sleep(0.1)

                splashed = whitepx()
                if splashed:
                    print("SPLASHED!")
                    click_it()
                    break
            break

        print('Debugging. x: {}, y: {}'.format(x, y))

    time.sleep(1)

print("Done Fishing!")