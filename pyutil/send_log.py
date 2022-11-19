import queue
import threading
import time
from collections import deque

import httpx

HOST = "localhost:3000"
THREADS = 8
REQUESTS = 100000
WINDOW = 5.0


class Requester(threading.Thread):
    def __init__(self, queue, out_queue):
        super().__init__()
        self.queue = queue
        self.out_queue = out_queue

    def run(self):
        while True:
            url = self.queue.get()
            if url is None:
                self.out_queue.put(None)
                return
            resp = httpx.get(f"http://{HOST}{url}", timeout=30)
            body_len = len(resp.read())
            self.out_queue.put((time.time(), resp.status_code, body_len))


def make_thread_pool(thread_count):
    url_queue = queue.Queue()
    out_queue = queue.Queue()
    threads = [Requester(url_queue, out_queue) for _ in range(thread_count)]
    for thread in threads:
        thread.start()
    return threads, url_queue, out_queue


def fill_url_queue(url_queue, request_count, log_name="cotl.xl0.org.log.cleaned"):
    requests_so_far = 0
    for line in open(log_name):
        url_queue.put(line.strip())
        requests_so_far += 1

        if requests_so_far == request_count:
            break


def main():
    threads, url_queue, out_queue = make_thread_pool(THREADS)
    fill_url_queue(url_queue, REQUESTS)

    for _ in range(THREADS):
        url_queue.put(None)

    resps = deque()
    done = 0
    while done < THREADS:
        try:
            q = out_queue.get(timeout=0.5)
        except queue.Empty:
            pass
        else:
            if q is None:
                done += 1
            else:
                resps.append(q)

        oldest = time.time() - WINDOW
        while resps and resps[0][0] < oldest:
            resps.popleft()

        total_size = 0
        for _, status, size in resps:
            total_size += size

        print(f"{len(resps)} req, {total_size/1024.0/1024.0} MB in {WINDOW:.2f} sec")


if __name__ == '__main__':
    main()
