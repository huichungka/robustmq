## 概述
RobustMQ MQTT 实现了共享订阅功能。共享订阅是一种订阅模式，用于在多个订阅者之间实现负载均衡。客户端可以分为多个订阅组，消息仍然会被转发到所有订阅组，但每个订阅组内只有一个客户端接收消息。您可以为一组订阅者的原始主题添加前缀以启用共享订阅。

RobustMQ MQTT 支持两种格式的共享订阅前缀，分别为带群组的共享订阅（前缀为 $share/<group-name>/）和不带群组的共享订阅（前缀为 $queue/）。两种共享订阅格式示例如下：
| 前缀格式 | 示例 | 前缀 | 真实主题名 |
| --- | --- | --- | --- |
| 带群组格式 | $share/abc/t/1 | $share/abc/ | t/1 |
| 不带群组格式 | $queue/t/1 | $queue/ | t/1 |

## 带群组的共享订阅
您可以通过在原始主题前添加 $share/<group-name> 前缀为分组的订阅者启用共享订阅。组名可以是任意字符串。RobustMQ MQTT Broker 同时将消息转发给不同的组，属于同一组的订阅者可以使用负载均衡接收消息。

例如，如果订阅者 s1、s2 和 s3 是组 g1 的成员，订阅者 s4 和 s5 是组 g2 的成员，而所有订阅者都订阅了原始主题 t1。共享订阅的主题必须是 $share/g1/t1 和 $share/g2/t1。当 RobustMQ MQTT Broker 发布消息 msg1 到原始主题 t1 时：

RobustMQ MQTT Broker 将 msg1 发送给 g1 和 g2 两个组。
s1、s2、s3 中的一个订阅者将接收 msg1。
s4 和 s5 中的一个订阅者将接收 msg1。
![image](../../images/share-sub-1.png)

## 不带群组的共享订阅
以 $queue/ 为前缀的共享订阅是不带群组的共享订阅。它是 $share 订阅的一种特例。您可以将其理解为所有订阅者都在一个订阅组中，如 $share/$queue。
![image](../../images/share-sub-2.png)

## 共享订阅与会话
当客户端具有持久会话并订阅了共享订阅时，会话将在客户端断开连接时继续接收发布到共享订阅主题的消息。如果客户端长时间断开连接且消息发布速率很高，会话状态中的内部消息队列可能会溢出。为了避免这个问题，建议为共享订阅使用 clean_session=true 的会话。即：会话在客户端断开连接后立即过期。

当客户端使用 MQTT v5 时，建议设置短会话过期时间（如果不是 0）。这样客户端可以暂时断开连接并重新连接以接收在断开连接期间发布的消息。当会话过期时，发送队列中的 QoS1 和 QoS2 消息，或者飞行窗口中的 QoS1 消息将被重新分发到同一组中的其他会话。当最后一个会话过期时，所有待处理的消息将被丢弃。
