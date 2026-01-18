
import asyncio
import argparse
import time
import random
import string

async def client_scenario(host, port, user_id, channel, msg_count, delay):
    reader, writer = await asyncio.open_connection(host, port)
    nick = f"load_{user_id}_{''.join(random.choices(string.ascii_lowercase, k=4))}"
    
    writer.write(f"NICK {nick}\r\nUSER {nick} 0 * :{nick}\r\n".encode())
    await writer.drain()

    # Simple loop to read initial burst/MOTD (imprecise)
    # real scenario would wait for logged in
    await asyncio.sleep(1) 
    
    writer.write(f"JOIN {channel}\r\n".encode())
    await writer.drain()
    await asyncio.sleep(1)

    for i in range(msg_count):
        msg = f"PRIVMSG {channel} :Load test message {i} from {nick}\r\n"
        writer.write(msg.encode())
        await writer.drain()
        await asyncio.sleep(delay)

    writer.write(b"QUIT :Done\r\n")
    await writer.drain()
    writer.close()
    await writer.wait_closed()

async def main():
    parser = argparse.ArgumentParser(description="IRC Load Tester")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=6667)
    parser.add_argument("--users", type=int, default=10)
    parser.add_argument("--messages", type=int, default=100)
    parser.add_argument("--channel", default="#loadtest")
    parser.add_argument("--rate", type=float, default=10.0, help="Messages per second per user")
    
    args = parser.parse_args()
    delay = 1.0 / args.rate

    print(f"Starting load test: {args.users} users, {args.messages} msgs each, {args.rate} msg/s/user")
    
    tasks = []
    for i in range(args.users):
        tasks.append(client_scenario(args.host, args.port, i, args.channel, args.messages, delay))
        # Stagger joins slightly
        await asyncio.sleep(0.05)

    start = time.time()
    await asyncio.gather(*tasks)
    duration = time.time() - start
    
    total_msgs = args.users * args.messages
    print(f"Finished. Total messages: {total_msgs}")
    print(f"Duration: {duration:.2f}s")
    print(f"Throughput: {total_msgs / duration:.2f} msg/s")

if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        pass
