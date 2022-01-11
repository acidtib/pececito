import time
import cv2
import numpy as np
import pyautogui
from pywinauto import Application
from PIL import ImageGrab
from random import randint, uniform

x = None
y = None

windowTitle = 'World of Warcraft'
matching_threshold = 0.6
white_pixel_threshold = 3
fishFor = 10

template = cv2.imread('template/day.png', 0)
w, h = template.shape[::-1]

print('Start fishing')
time.sleep(5)

# bring game window forward
wow = Application().connect(path="WoW.exe", title=windowTitle)
print('Focusing game window!') 
wow.WorldofWarcraft.set_focus()

def ran(max=600):
    return randint(0, max)

def ranFloat(max=0.9):
    return round(uniform(0, max), 2)

def whitepx():
    current = cv2.cvtColor(np.array(cv2.imread('catch/hover_bob.png')), cv2.COLOR_BGRA2GRAY)
    cv2.imwrite('catch/hover_bob_clean.png', current)
    H_B, W_B = current.shape
    white_pixels = 0
    i = 0
    while i < H_B:
        j = 0
        # print('BOB Splash. x: {}, y: {}, pixel: {}'.format(x, y, white_pixels))
        while j < W_B:
            if current[i][j] > 245:
                white_pixels = white_pixels + 1
            print('BOB Splash. x: {}, y: {}, pixel: {}, threshold: {}'.format(x, y, white_pixels, white_pixel_threshold))
            if white_pixels > white_pixel_threshold:                
                return True
            j = j + 1
        i = i + 1
    return False

def cast_it():
    print('Casting line!') 
    pyautogui.typewrite("0")  # Cast fishing skill on 0
    time.sleep(ran(2))

def click_it():
    pyautogui.keyDown('shift')
    
    time.sleep(ranFloat(2.5))
    pyautogui.mouseDown(button='right')
    # time.sleep(ranFloat(0.8))
    
    pyautogui.mouseUp(button='right')
    time.sleep(ranFloat(0.3))
    pyautogui.keyUp('shift')


for fishing in range(fishFor):
    pyautogui.moveTo(ran(), ran())
    x = None
    y = None

    cast_it()

    for search_bob in range(4):
        if x is None:
            time.sleep(1)
            # TODO: improve base screenshot, only capture game window
            base_screen = ImageGrab.grab(bbox=(0, 0, 1920, 1080))
            base_screen.save('catch/base_screen.png')  # Save to file

            img_rgb = cv2.imread('catch/base_screen.png') # Read base
            img_gray = cv2.cvtColor(img_rgb, cv2.COLOR_BGR2GRAY)

            res = cv2.matchTemplate(img_gray, template, cv2.TM_CCOEFF_NORMED) # Search for match
            loc = np.where(res >= matching_threshold)

            for pt in zip(*loc[::-1]):
                x = int(pt[0])
                y = int(pt[1])
                cv2.rectangle(img_rgb, pt, (pt[0] + w, pt[1] + h), (0,255,255), 1)
                cv2.imwrite('catch/base_matched.png', img_rgb)
        else:
            clean_bob = ImageGrab.grab(bbox=(x, y, x + w, y + h))
            clean_bob.save('catch/clean_bob.png')

            # move mouse to bob
            pyautogui.moveTo(x + w / 2, y + h / 2, 1, pyautogui.easeOutQuad)

            for bite in range(220):
                hover_bob = ImageGrab.grab(bbox=(x, y, x + w, y + h))
                hover_bob.save('catch/hover_bob.png')
                # time.sleep(0.1)

                # did it splash
                if whitepx():
                    click_it()
                    print("SPLASHED!")
                    break
            break

        print('Looking for BOB. x: {}, y: {}'.format(x, y))

    time.sleep(ran(3))

print("Done Fishing!")